import assert from 'node:assert/strict';
import childProcess from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { syncBuiltinESMExports } from 'node:module';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { test } from 'node:test';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';
import { openSnapshotSession, VecnookError } from './snapshot-session.mjs';

const exe = process.platform === 'win32' ? 'vecnook.exe' : 'vecnook';
const binary = process.env.VECNOOK_BINARY ? resolve(process.env.VECNOOK_BINARY)
  : fileURLToPath(new URL(`../../target/release/${exe}`, import.meta.url));
const id = '18446744073709551615';
const original = '원문\n두 번째 🚀';
const isKind = kind => error => error instanceof VecnookError && error.kind === kind;

async function fixture(t, metric = 'l2') {
  const root = await mkdtemp(join(tmpdir(), 'vecnook-node-session-'));
  const db = join(root, 'raw db');
  const store = join(root, 'generation store');
  const cache = join(root, 'cache');
  const readers = [];
  t.after(async () => {
    await Promise.allSettled(readers.map(reader => reader.close()));
    await rm(root, { recursive: true, force: true });
  });
  const run = (...args) => {
    const process = childProcess.spawnSync(binary, ['--json-errors', ...args], {
      encoding: 'utf8', cwd: root, timeout: 15_000, windowsHide: true,
    });
    if (process.error) throw process.error;
    return process;
  };
  const native = (...args) => {
    const result = run(...args);
    assert.equal(result.status, 0, result.stderr);
    return result.stdout;
  };
  native('init', db, '2', '--metric', metric);
  native('put', db, id, '1,-0.0', original);
  native('put', db, '1', '0,1', 'other');
  native('snapshot-publish', db, store, 'node-session');
  return { db, store, cache, run, native, open: async options => {
    const reader = await openSnapshotSession(store, { binary, timeoutMs: 15_000, ...options });
    readers.push(reader);
    return reader;
  } };
}

test('runs the local snapshot app through publication and reopen', () => {
  const script = fileURLToPath(new URL('./local-snapshot-app.mjs', import.meta.url));
  const app = childProcess.spawnSync(process.execPath, [script], {
    env: { ...process.env, VECNOOK_BINARY: binary },
    encoding: 'utf8', timeout: 45_000, windowsHide: true,
  });
  if (app.error) throw app.error;
  assert.equal(app.status, 0, app.stderr);
  const rows = app.stdout.trim().split(/\r?\n/).map(line => JSON.parse(line));
  assert.deepEqual(rows.map(row => row.stage), ['initial', 'initial', 'pinned', 'reopened']);
  for (const row of rows) assert.deepEqual(row.neighbors.map(match => match.id), [id]);
  const [first, second, pinned, reopened] = rows;
  assert.equal(first.neighbors[0].distance, 0);
  assert.ok(second.neighbors[0].distance > 0);
  assert.equal(first.generation, second.generation);
  assert.equal(first.generation, pinned.generation);
  assert.notEqual(pinned.generation, reopened.generation);
  assert.deepEqual(rows.map(row => row.neighbors[0].metadata),
    ['Original note', 'Original note', 'Original note', 'Revised note']);
});

test('streams one filtered query at a time and recovers after a native query error', async t => {
  const { open } = await fixture(t);
  const reader = await open({ sq8: 16 });
  const ready = reader.info();
  assert.equal(ready.snapshot_opens, 1);
  assert.equal(ready.sq8_builds, 1);
  let pulled = 0;
  async function* vectors() {
    for (const vector of [new Float32Array([1, -0]), [1], [0, 1]]) {
      pulled++;
      yield vector;
    }
  }
  const batch = reader.searchBatch(vectors(), 1, { ids: [BigInt(id)] });
  assert.equal(pulled, 0);
  const reply = (await batch.next()).value;
  assert.equal(pulled, 1);
  assert.equal(reply.result.generation, ready.generation);
  assert.equal(reply.result.sequence, ready.sequence);
  assert.equal(reply.snapshot_opens, 1);
  assert.equal(reply.sq8_builds, 1);
  assert.deepEqual(reply.result.neighbors, [{ id, distance: 0, metadata: original }]);
  await assert.rejects(batch.next(), isKind('InvalidInput'));
  assert.equal(pulled, 2);
  assert.equal((await batch.next()).done, true);
  await assert.rejects(reader.searchVector([1, 0], 1, { ids: [Number(id)] }), TypeError);
  await assert.rejects(reader.searchVector([1, 0], 1, { ids: [`${id}\n`] }), TypeError);
  await assert.rejects(reader.searchVector([1e100, 0], 1), TypeError);
  const empty = await reader.searchVector([1, 0], 1, { ids: [] });
  assert.deepEqual(empty.result.neighbors, []);
  const pending = reader.searchVector([0, 1], 1, { ids: ['1'] });
  await assert.rejects(reader.searchVector([1, 0], 1), isKind('Busy'));
  const closing = reader.close();
  assert.equal(reader.close(), closing);
  assert.equal((await pending).result.neighbors[0].id, '1');
  await closing;
  await assert.rejects(reader.searchVector([1, 0], 1), isKind('Closed'));
});

test('pins the generation and a prepared cache until close', async t => {
  const { db, store, cache, native, run, open } = await fixture(t);
  native('snapshot-gc', store);
  native('snapshot-prepare-sq8', store, cache);
  const old = await open({ sq8: 16, sq8Cache: cache, bufferBytes: 36 });
  assert.equal(old.info().cache_hits, 1);
  assert.equal(old.info().sq8_builds, 0);
  assert.equal(old.info().code_read_buffer_bytes, 36);
  const locked = run('snapshot-prune-sq8', cache);
  assert.equal(locked.status, 1);
  assert.equal(JSON.parse(locked.stderr).error.kind, 'Locked');
  native('put', db, id, '1,-0.0', 'updated original');
  native('snapshot-publish', db, store, 'node-session');
  const fresh = await open();
  assert.notEqual(old.info().generation, fresh.info().generation);
  assert.equal(JSON.parse(native('snapshot-gc', store)).removed_generations, 0);
  const results = await Promise.all([old, fresh].map(r => r.searchVector([1, 0], 1, { ids: [id] })));
  assert.deepEqual(results.map(r => r.result.neighbors[0].metadata), [original, 'updated original']);
  await old.close();
  assert.ok(JSON.parse(native('snapshot-prune-sq8', cache)).removed_files > 0);
  assert.equal(JSON.parse(native('snapshot-gc', store)).removed_generations, 1);
});

test('preserves the native negative-zero distance and reports idle expiry', async t => {
  const { store, native, open } = await fixture(t, 'ip');
  const direct = JSON.parse(native('snapshot-search', store, '0,1', '2'));
  assert.ok(Object.is(direct.neighbors[1].distance, -0));
  const reader = await open();
  const { result } = await reader.searchVector([0, 1], 2);
  assert.deepEqual(result, direct);
  await reader.close();
  const idle = await open({ idleMs: 100 });
  const deadline = performance.now() + 10_000;
  while (!idle.closed && performance.now() < deadline) await delay(25);
  assert.equal(idle.closed, true);
  await assert.rejects(idle.searchVector([0, 1], 1), isKind('IdleTimeout'));
  await idle.close();
});

test('reports a structured native startup failure', async t => {
  const { store, run } = await fixture(t);
  const missing = join(store, 'missing');
  const direct = run('snapshot-session', missing);
  assert.notEqual(direct.status, 0);
  await assert.rejects(openSnapshotSession(missing, { binary, timeoutMs: 15_000 }),
    isKind(JSON.parse(direct.stderr).error.kind));
});

// Substitute a local Node child at the spawn boundary to exercise broken framing
// and I/O without a shell, an alternate CLI binary or test hooks in the bridge.
async function fakeSession(t, script, options = {}) {
  const spawn = childProcess.spawn;
  let child;
  const stub = t.mock.method(childProcess, 'spawn', (_binary, _args, spawnOptions) => {
    child = spawn(process.execPath, ['-e', script], spawnOptions);
    return child;
  });
  syncBuiltinESMExports();
  t.after(() => {
    stub.mock.restore();
    syncBuiltinESMExports();
    child?.kill('SIGKILL');
  });
  const reader = await openSnapshotSession('unused', { timeoutMs: 10_000, ...options });
  return { child, reader };
}

const readyScript = `
  const send = value => process.stdout.write(JSON.stringify({schema_version:1, ...value}) + '\\n');
  send({event:'ready', generation:'1', sequence:'2', snapshot_opens:1, sq8_builds:0});
`;

test('preserves a signed zero in the request and reaps a child that crosses generations', async t => {
  const { child, reader } = await fakeSession(t, readyScript + `
    require('node:readline').createInterface({input:process.stdin}).once('line', line => {
      const request = JSON.parse(line);
      const result = {schema_version:1, generation:'9', sequence:'2', neighbors:[]};
      if (!Object.is(request.vector[1], -0)) result.generation = '1';
      send({request_id:request.request_id, snapshot_opens:1, sq8_builds:0, result});
    });
  `);
  await assert.rejects(reader.searchVector([1, -0], 1), isKind('ProtocolError'));
  assert.ok(child.exitCode !== null || child.signalCode !== null);
  assert.equal(reader.closed, true);
  await reader.close();
});

test('enforces a response bound before a newline arrives', async t => {
  const { child, reader } = await fakeSession(t, readyScript + `
    process.stdin.once('data', () => process.stdout.write('x'.repeat(2048)));
  `, { maxResponseBytes: 1024 });
  await assert.rejects(reader.searchVector([1, 0], 1), isKind('ProtocolError'));
  assert.ok(child.exitCode !== null || child.signalCode !== null);
  await reader.close();
});

test('times out and reaps a child with a blocked request pipe', async t => {
  const { child, reader } = await fakeSession(t, readyScript + 'setInterval(() => {}, 60_000);',
    { timeoutMs: 3000 });
  await assert.rejects(reader.searchVector([1, 0], 1, { ids: Array(40_000).fill(id) }),
    isKind('Timeout'));
  assert.ok(child.exitCode !== null || child.signalCode !== null);
  await reader.close();
});
