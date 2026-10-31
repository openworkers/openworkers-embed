// Bun host of the layer ladder: the same workers as examples/ladder.rs.
//
//   bun benches/rps/ladder-bun.ts raw            Bun.serve answers "hello"
//   bun benches/rps/ladder-bun.ts <worker.js>    Bun.serve in front of the worker

import { resolve } from 'node:path';

const [mode] = process.argv.slice(2);
const port = Number(process.env.PORT ?? 3000);
const env = { EMBED: { invoke: async () => [{ x: 1 }] } };
const ctx = { waitUntil() {}, passThroughOnException() {} };

const worker =
  mode === 'raw'
    ? { fetch: () => new Response('hello') }
    : (await import(resolve(mode))).default;

Bun.serve({
  port,
  hostname: '127.0.0.1',
  fetch: (request) => worker.fetch(request, env, ctx),
});

console.log(`listening on http://127.0.0.1:${port}`);
