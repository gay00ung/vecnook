import { execFile } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { promisify } from 'node:util';
import { openSnapshotSession } from './snapshot-session.mjs';

const binary = process.env.VECNOOK_BINARY ? resolve(process.env.VECNOOK_BINARY) : 'vecnook';
const run = promisify(execFile);
const workspace = await mkdtemp(join(tmpdir(), 'vecnook-local-snapshot-'));
const database = join(workspace, 'raw db');
const store = join(workspace, 'generation store');
const id = '18446744073709551615';
const filter = { ids: [id] };

const native = (...args) => run(binary, ['--json-errors', ...args], {
  cwd: workspace, timeout: 15_000, windowsHide: true,
});
const show = (stage, { result }) => {
  console.log(JSON.stringify({ stage, generation: result.generation, neighbors: result.neighbors }));
};

try {
  await native('init', database, '2', '--metric', 'l2');
  await native('put', database, id, '1,0', 'Original note');
  await native('put', database, '2', '0,1', 'Other note');
  await native('snapshot-publish', database, store, 'node-local-app');

  const reader = await openSnapshotSession(store, { binary });
  try {
    for await (const reply of reader.searchBatch([[1, 0], [0, 1]], 2, filter)) {
      show('initial', reply);
    }

    await native('put', database, id, '1,0', 'Revised note');
    await native('snapshot-publish', database, store, 'node-local-app');
    show('pinned', await reader.searchVector([1, 0], 2, filter));
  } finally {
    await reader.close();
  }

  const reopened = await openSnapshotSession(store, { binary });
  try {
    show('reopened', await reopened.searchVector([1, 0], 2, filter));
  } finally {
    await reopened.close();
  }
} finally {
  await rm(workspace, { recursive: true, force: true });
}
