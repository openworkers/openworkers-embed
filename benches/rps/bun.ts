// Bun host of the rock-paper-scissors bench: Bun.serve in front, the worker
// of the build behind, and bun:sqlite for platform.invoke('query').
//
//   bun benches/rps/bun.ts <build-dir> <database>

import { Database } from 'bun:sqlite';
import { join } from 'node:path';

const [build, database] = process.argv.slice(2);
const port = Number(process.env.PORT ?? 3000);
const db = new Database(database, { readonly: true });

function query({ sql, params }: { sql: string; params: any[] }) {
  return db.query(sql).all(...params);
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
  fetch: (request) => worker.fetch(request, env, ctx),
});

console.log(`listening on http://127.0.0.1:${port}`);
