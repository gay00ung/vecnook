// Node.js standard-library bridge. Run: node examples/javascript/search.mjs DB QUERY
// Calls are asynchronous, but each CLI call acquires the exclusive collection lock.
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
const run = promisify(execFile);
const [db, query] = process.argv.slice(2);
if (!db || !query) throw new Error('Usage: search.mjs collection-dir query');
const { stdout } = await run(process.env.VECNOOK_BINARY || 'vecnook',
  ['--json-errors', 'docs-find', query, '--db', db, '--mode', 'keyword', '--json'],
  { encoding: 'utf8', maxBuffer: 16 * 1024 * 1024, timeout: 120000 });
const result = JSON.parse(stdout);
for (const match of result.neighbors) {
  // Keep the canonical decimal string, including IDs above Number.MAX_SAFE_INTEGER.
  if (typeof match.id !== 'string') throw new Error('Invalid ID contract');
  console.log(JSON.stringify({ id: match.id, source: match.source,
    line: match.start_line, text: match.text, score: match.score }));
}
