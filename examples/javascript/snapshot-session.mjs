import { spawn } from 'node:child_process';
import { TextDecoder } from 'node:util';

const MIB = 1024 * 1024;
const MAX_U64_TEXT = '18446744073709551615';
const MAX_U64 = BigInt(MAX_U64_TEXT);
const decoder = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });

export class VecnookError extends Error {
  constructor(kind, message, options) {
    super(message, options);
    this.name = 'VecnookError';
    this.kind = kind;
  }
}

function u64String(id) {
  return typeof id === 'string' && /^(?:0|[1-9][0-9]{0,19})$/.test(id)
    && (id.length < 20 || id <= MAX_U64_TEXT);
}

function integer(value, maximum, name) {
  if (!Number.isSafeInteger(value) || value < 1 || value > maximum) {
    throw new RangeError(`${name} must be an integer in 1..${maximum}`);
  }
  return value;
}

function pathString(value, name) {
  if (typeof value !== 'string' || !value || value.includes('\0')) {
    throw new TypeError(`${name} must be a nonempty path string`);
  }
  return value;
}

function nativeError(error) {
  if (typeof error?.kind === 'string' && typeof error.message === 'string') {
    return new VecnookError(error.kind, error.message);
  }
  return null;
}

function searchOptions(k, ids) {
  integer(k, 10_000, 'k');
  if (ids === undefined) return { k };
  const length = Array.isArray(ids) ? ids.length : undefined;
  if (!Number.isSafeInteger(length) || length < 0 || length > 65_536) {
    throw new TypeError('ids must be an array of at most 65536 IDs');
  }
  const canonicalIds = Array.from({ length }, (_, index) => {
    const id = ids[index];
    if (typeof id === 'bigint' && id >= 0n && id <= MAX_U64) return id.toString();
    if (u64String(id)) return id;
    throw new TypeError('IDs must be canonical decimal strings or u64 BigInts');
  });
  return { k, ids: canonicalIds };
}

function searchFrame(requestId, vector, options) {
  const length = Array.isArray(vector) || vector instanceof Float32Array ? vector.length : undefined;
  if (!Number.isSafeInteger(length) || length < 1 || length > 4096) {
    throw new TypeError('vector must contain 1..4096 coordinates');
  }
  const coordinates = Array.from({ length }, (_, index) => {
    const value = vector[index];
    const coordinate = typeof value === 'number' ? Math.fround(value) : NaN;
    if (!Number.isFinite(coordinate)) {
      throw new TypeError('vector coordinates must be finite f32 values');
    }
    return Object.is(coordinate, -0) ? '-0' : String(coordinate);
  });
  // JSON.stringify writes -0 as 0. Encode the validated f32 coordinates directly.
  const fields = JSON.stringify({ request_id: requestId, ...options });
  const line = `${fields.slice(0, -1)},"vector":[${coordinates.join(',')}]}\n`;
  if (Buffer.byteLength(line) > MIB) throw new RangeError('request exceeds 1 MiB');
  return line;
}

export async function openSnapshotSession(store, {
  binary = process.env.VECNOOK_BINARY || 'vecnook',
  sq8, sq8Cache, bufferBytes,
  idleMs = 60_000, timeoutMs = 30_000, maxResponseBytes = 16 * MIB,
} = {}) {
  const args = ['--json-errors', 'snapshot-session', pathString(store, 'store'),
    '--idle-ms', String(integer(idleMs, 86_400_000, 'idleMs'))];
  pathString(binary, 'binary');
  integer(timeoutMs, 86_400_000, 'timeoutMs');
  integer(maxResponseBytes, 64 * MIB, 'maxResponseBytes');
  if ((sq8Cache !== undefined && sq8 === undefined)
      || (bufferBytes !== undefined && sq8Cache === undefined)) {
    throw new TypeError('sq8Cache requires sq8, and bufferBytes requires sq8Cache');
  }
  if (sq8 !== undefined) args.push('--sq8', String(integer(sq8, 65_536, 'sq8')));
  if (sq8Cache !== undefined) args.push('--sq8-cache', pathString(sq8Cache, 'sq8Cache'));
  if (bufferBytes !== undefined) {
    args.push('--buffer-bytes', String(integer(bufferBytes, Number.MAX_SAFE_INTEGER, 'bufferBytes')));
  }
  const child = spawn(binary, args, { stdio: 'pipe', windowsHide: true });
  const session = new SnapshotSession(child, timeoutMs, maxResponseBytes);
  await session.ready();
  return session;
}

class SnapshotSession {
  #child;
  #timeoutMs;
  #maxResponseBytes;
  #exit;
  #pending;
  #failure;
  #closing;
  #closeAcknowledged = false;
  #flight;
  #info;
  #requestId = 0n;
  #chunks = [];
  #buffered = 0;

  constructor(child, timeoutMs, maxResponseBytes) {
    this.#child = child;
    this.#timeoutMs = timeoutMs;
    this.#maxResponseBytes = maxResponseBytes;
    let stderr = Buffer.alloc(0);
    child.stderr.on('data', chunk => {
      if (stderr.length < 16 * 1024) {
        stderr = Buffer.concat([stderr, chunk.subarray(0, 16 * 1024 - stderr.length)]);
      }
    });
    this.#exit = new Promise(resolve => {
      child.once('close', (code, signal) => {
        if (!this.#closeAcknowledged || code !== 0) {
          let error;
          try {
            const envelope = JSON.parse(decoder.decode(stderr));
            if (envelope?.schema_version === 1) error = nativeError(envelope.error);
          } catch {
            // The child can exit without a structured diagnostic.
          }
          this.#fail(error ?? new VecnookError('ProcessExited',
            `snapshot-session exited (${signal ?? code})`));
        }
        resolve();
      });
    });
    const onError = cause => this.#fail(new VecnookError('TransportError',
      cause.message, { cause }));
    for (const emitter of [child, child.stdin, child.stdout, child.stderr]) {
      emitter.on('error', onError);
    }
    child.stdout.on('data', chunk => this.#read(chunk));
  }

  async ready() {
    if (this.#info) return this.#info;
    this.#info = Object.freeze(await this.#exchange('ready'));
    return this.#info;
  }

  info() {
    return { ...this.#info };
  }

  get closed() {
    return Boolean(this.#failure || this.#closing);
  }

  async searchVector(vector, k, { ids } = {}) {
    return this.#search(vector, searchOptions(k, ids));
  }

  async *searchBatch(vectors, k, { ids } = {}) {
    const options = searchOptions(k, ids);
    for await (const vector of vectors) yield await this.#search(vector, options);
  }

  async #search(vector, options) {
    if (this.closed) {
      throw this.#failure ?? new VecnookError('Closed', 'snapshot session is closed');
    }
    if (this.#flight) throw new VecnookError('Busy', 'await the current search on this session');
    const id = String(++this.#requestId);
    this.#flight = this.#exchange('search', id, searchFrame(id, vector, options));
    try {
      return await this.#flight;
    } finally {
      this.#flight = undefined;
    }
  }

  close() {
    return this.#closing ??= this.#shutdown();
  }

  async #shutdown() {
    await this.#flight?.catch(() => {});
    if (this.#failure) {
      await this.#exit;
      return;
    }
    const id = String(++this.#requestId);
    await this.#exchange('closed', id, `{"request_id":"${id}","op":"close"}\n`);
    this.#child.stdin.end();
    const timer = setTimeout(() => this.#fail(new VecnookError('Timeout',
      'snapshot-session did not exit after close')), this.#timeoutMs);
    try {
      await this.#exit;
      if (this.#failure) throw this.#failure;
    } finally {
      clearTimeout(timer);
    }
  }

  async #exchange(kind, id, line) {
    try {
      return await new Promise((resolve, reject) => {
        const timer = setTimeout(() => this.#fail(new VecnookError('Timeout',
          `timed out waiting for snapshot-session ${kind}`)), this.#timeoutMs);
        this.#pending = { kind, id, resolve, reject, timer };
        if (line) this.#child.stdin.write(line, error => {
          if (error) this.#fail(new VecnookError('TransportError', error.message, { cause: error }));
        });
      });
    } catch (error) {
      if (this.#failure) await this.#exit;
      throw error;
    }
  }

  #settle(error, reply) {
    const pending = this.#pending;
    if (!pending) return;
    clearTimeout(pending.timer);
    this.#pending = undefined;
    if (error) pending.reject(error);
    else pending.resolve(reply);
  }

  #fail(error) {
    if (this.#failure) return;
    this.#failure = error;
    this.#chunks = [];
    this.#settle(error);
    this.#child.stdin.destroy();
    if (this.#child.exitCode === null && this.#child.signalCode === null) {
      this.#child.kill('SIGKILL');
    }
  }

  #read(chunk) {
    for (let start = 0; start < chunk.length && !this.#failure;) {
      const newline = chunk.indexOf(10, start);
      const end = newline < 0 ? chunk.length : newline;
      const part = chunk.subarray(start, end);
      this.#buffered += part.length;
      if (this.#buffered >= this.#maxResponseBytes) {
        this.#fail(new VecnookError('ProtocolError', 'response exceeds maxResponseBytes'));
        return;
      }
      this.#chunks.push(part);
      if (newline < 0) return;
      const line = Buffer.concat(this.#chunks, this.#buffered);
      this.#chunks = [];
      this.#buffered = 0;
      try {
        this.#receive(JSON.parse(decoder.decode(line)));
      } catch (cause) {
        this.#fail(new VecnookError('ProtocolError', cause.message, { cause }));
      }
      start = newline + 1;
    }
  }

  #receive(reply) {
    if (reply?.schema_version !== 1) throw new Error('invalid session response schema');
    if (reply.event === 'idle_timeout' && !this.#closeAcknowledged) {
      this.#fail(new VecnookError('IdleTimeout', 'snapshot-session idle time expired'));
      return;
    }
    const pending = this.#pending;
    if (!pending) throw new Error('unsolicited session response');
    if (pending.kind === 'ready') {
      if (reply.event !== 'ready' || !u64String(reply.generation) || !u64String(reply.sequence)
          || reply.snapshot_opens !== 1 || ![0, 1].includes(reply.sq8_builds)) {
        throw new Error('invalid session ready event');
      }
    } else {
      if (reply.request_id !== pending.id) throw new Error('wrong session request ID');
      if (pending.kind === 'closed') {
        if (reply.event !== 'closed') throw new Error('expected session close event');
        this.#closeAcknowledged = true;
      } else {
        const error = nativeError(reply.error);
        if (error && reply.event === undefined && reply.result === undefined) {
          this.#settle(error);
          return;
        }
        const result = reply.result;
        if (reply.event !== undefined || reply.error !== undefined
            || result?.schema_version !== 1 || result.generation !== this.#info.generation
            || result.sequence !== this.#info.sequence
            || reply.snapshot_opens !== this.#info.snapshot_opens
            || reply.sq8_builds !== this.#info.sq8_builds || !Array.isArray(result.neighbors)
            || result.neighbors.some(n => !u64String(n?.id) || !Number.isFinite(n.distance)
              || typeof n.metadata !== 'string')) {
          throw new Error('invalid session search response or generation');
        }
      }
    }
    this.#settle(null, reply);
  }
}
