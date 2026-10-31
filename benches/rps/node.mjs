// Node.js host of the rock-paper-scissors bench: node:http in front, the
// worker of the build behind, and node:sqlite for platform.invoke('query').
//
//   node benches/rps/node.mjs <build-dir> <database>

import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { DatabaseSync } from 'node:sqlite';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

const [build, database] = process.argv.slice(2);
const port = Number(process.env.PORT ?? 3000);
const db = new DatabaseSync(database, { readOnly: true });
const statements = new Map();

// With MEMO=1, each query runs once and its rows are kept, as in examples/rps.rs
const memo = process.env.MEMO === '1' ? new Map() : null;

function query(args) {
  if (!memo) {
    return execute(args);
  }

  const key = `${args.sql}\n${JSON.stringify(args.params)}`;
  let rows = memo.get(key);

  if (!rows) {
    rows = execute(args);
    memo.set(key, rows);
  }

  // The same rows each time: the most favorable case for the host
  return rows;
}

function execute({ sql, params }) {
  let statement = statements.get(sql);

  if (!statement) {
    statement = db.prepare(sql);
    statements.set(sql, statement);
  }

  return statement.all(...params);
}

const env = {
  EMBED: { invoke: async (command, args) => query(args) },
  ASSETS: {
    async fetch(url) {
      const path = new URL(url, 'http://localhost').pathname;

      try {
        return new Response(await readFile(join(build, 'assets', path)));
      } catch {
        return new Response('not found', { status: 404 });
      }
    },
  },
};
const ctx = { waitUntil() {}, passThroughOnException() {} };

globalThis.env = env;

const worker = (await import(pathToFileURL(join(build, '_worker.js')))).default;

createServer(async (req, res) => {
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
}).listen(port, '127.0.0.1', () => console.log(`listening on http://127.0.0.1:${port}`));
