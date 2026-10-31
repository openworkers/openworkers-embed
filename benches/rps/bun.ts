// Bun host of the rock-paper-scissors bench: Bun.serve in front, the worker
// of the build behind, and bun:sqlite for platform.invoke('query').
//
//   bun benches/rps/bun.ts <build-dir> <database>

import { Database } from 'bun:sqlite';
import { join } from 'node:path';

const [build, database] = process.argv.slice(2);
const port = Number(process.env.PORT ?? 3000);
const db = new Database(database, { readonly: true });

// With MEMO=1, each query runs once and its rows are kept, as in examples/rps.rs
const memo = process.env.MEMO === '1' ? new Map<string, unknown[]>() : null;

function query({ sql, params }: { sql: string; params: any[] }) {
  if (!memo) {
    return db.query(sql).all(...params);
  }

  const key = `${sql}\n${JSON.stringify(params)}`;
  let rows = memo.get(key);

  if (!rows) {
    rows = db.query(sql).all(...params);
    memo.set(key, rows);
  }

  // The same rows each time: the most favorable case for the host
  return rows;
}

const env = {
  EMBED: { invoke: async (_command: string, args: any) => query(args) },
  ASSETS: {
    async fetch(url: string) {
      const file = Bun.file(join(build, 'assets', new URL(url, 'http://localhost').pathname));

      return (await file.exists()) ? new Response(file) : new Response('not found', { status: 404 });
    },
  },
};
const ctx = { waitUntil() {}, passThroughOnException() {} };

(globalThis as any).env = env;

const worker = (await import(join(build, '_worker.js'))).default;

Bun.serve({
  port,
  hostname: '127.0.0.1',
  // REUSE_PORT=1 lets several processes listen on the port (see bun-n.sh)
  reusePort: process.env.REUSE_PORT === '1',
  fetch: (request) => worker.fetch(request, env, ctx),
});

console.log(`listening on http://127.0.0.1:${port}`);
