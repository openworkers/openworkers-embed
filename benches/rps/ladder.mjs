// Node.js host of the layer ladder: the same workers as examples/ladder.rs.
//
//   node benches/rps/ladder.mjs raw            node:http answers "hello"
//   node benches/rps/ladder.mjs <worker.js>    node:http in front of the worker

import { createServer } from 'node:http';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const [mode] = process.argv.slice(2);
const port = Number(process.env.PORT ?? 3000);
const env = { EMBED: { invoke: async () => [{ x: 1 }] } };
const ctx = { waitUntil() {}, passThroughOnException() {} };

let handle;

if (mode === 'raw') {
  handle = (req, res) => res.end('hello');
} else {
  const worker = (await import(pathToFileURL(resolve(mode)))).default;

  handle = async (req, res) => {
    const headers = new Headers();

    for (let i = 0; i < req.rawHeaders.length; i += 2) {
      headers.append(req.rawHeaders[i], req.rawHeaders[i + 1]);
    }

    const request = new Request(`http://${req.headers.host}${req.url}`, {
      method: req.method,
      headers,
    });
    const response = await worker.fetch(request, env, ctx);

    res.writeHead(response.status, [...response.headers]);

    if (response.body) {
      for await (const chunk of response.body) {
        res.write(chunk);
      }
    }

    res.end();
  };
}

createServer(handle).listen(port, '127.0.0.1', () => console.log(`listening on http://127.0.0.1:${port}`));
