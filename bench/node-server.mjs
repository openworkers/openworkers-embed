// The bench app on Node: node:http, the SvelteKit server of build-node, and
// platform.invoke answered by node:sqlite on the same thread.
//
//   node node-server.mjs            page and JSON route, port 3001
//   MODE=hello node node-server.mjs a fixed body
import { createServer } from 'node:http';
import { DatabaseSync } from 'node:sqlite';
import { LEADERS, RECENT, TOTALS } from './queries.mjs';

const port = Number(process.env.PORT ?? 3001);

if (process.env.MODE === 'hello') {
  createServer((req, res) => res.end('hello')).listen(port);
} else {
  const { Server } = await import('./app/build-node/server/index.js');
  const { manifest } = await import('./app/build-node/server/manifest.js');
  const { getRequest, setResponse } = await import('./app/node_modules/@sveltejs/kit/src/exports/node/index.js');

  const db = new DatabaseSync(new URL('rps.db', import.meta.url).pathname, { readOnly: true });
  const leaders = db.prepare(LEADERS);
  const recent = db.prepare(RECENT);
  const totals = db.prepare(TOTALS);
  const commands = {
    leaders: () => leaders.all(),
    recent: () => recent.all(),
    totals: () => totals.get(),
  };
  const platform = { invoke: async (name) => commands[name]() };

  const server = new Server(manifest);
  await server.init({ env: process.env });

  createServer(async (req, res) => {
    const request = await getRequest({ request: req, base: `http://${req.headers.host}` });
    const response = await server.respond(request, {
      platform,
      getClientAddress: () => req.socket.remoteAddress,
    });

    await setResponse(res, response);
  }).listen(port);
}

console.log(`listening on http://127.0.0.1:${port}`);
