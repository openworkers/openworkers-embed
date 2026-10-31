// The bench app on Bun: Bun.serve, the SvelteKit server of build-node, and
// platform.invoke answered by bun:sqlite on the same thread.
//
//   bun bun-server.ts             page and JSON route, port 3002
//   MODE=hello bun bun-server.ts  a fixed body
import { Database } from 'bun:sqlite';
import { LEADERS, RECENT, TOTALS } from './queries.mjs';

const port = Number(process.env.PORT ?? 3002);

if (process.env.MODE === 'hello') {
  Bun.serve({ port, fetch: () => new Response('hello') });
} else {
  const { Server } = await import('./app/build-node/server/index.js');
  const { manifest } = await import('./app/build-node/server/manifest.js');

  const db = new Database(new URL('rps.db', import.meta.url).pathname, { readonly: true });
  const leaders = db.query(LEADERS);
  const recent = db.query(RECENT);
  const totals = db.query(TOTALS);
  const commands: Record<string, () => unknown> = {
    leaders: () => leaders.all(),
    recent: () => recent.all(),
    totals: () => totals.get(),
  };
  const platform = { invoke: async (name: string) => commands[name]() };

  const server = new Server(manifest);
  await server.init({ env: process.env });

  Bun.serve({
    port,
    fetch: (request, bun) =>
      server.respond(request, {
        platform,
        getClientAddress: () => bun.requestIP(request)?.address ?? '',
      }),
  });
}

console.log(`listening on http://127.0.0.1:${port}`);
