# The cost of one request at one thread

This file holds the measures of the branch `experiment/warm-places`. It ranks
the costs of one request on one worker thread of Embed. It compares them with
Node.js and Bun on the same page.

## The machine

The measures of this file come from a cloud VM, not from the Apple M5 Max of
CLAUDE.md:

- Linux 6.18, 4 vCPUs, 15 GB of memory.
- Node.js 22.22.0, Bun 1.4.4, oha 1.16.0.
- A release build of `examples/bench_server.rs`, one worker thread.

The VM is slower than the M5 Max. Compare the servers with each other on one
machine. Do not compare the numbers of this file with the numbers of
CLAUDE.md.

## The bench

`bench/` holds a SvelteKit app and the three servers that serve it:

- `bench/app`: page `/` renders a board from three reads. Route `/api/stats`
  answers the same three reads as JSON, with no render. Each read is one
  `platform.invoke`.
- `bench/seed.mjs` writes `bench/rps.db`: 1000 players and 20000 games.
- `examples/bench_server.rs`: Embed behind Axum. The commands read SQLite
  with rusqlite.
- `bench/node-server.mjs`: `node:http`, the SvelteKit server of the
  `adapter-node` build, and `node:sqlite`.
- `bench/bun-server.ts`: `Bun.serve`, the same SvelteKit server, and
  `bun:sqlite`.

The three servers run the same SvelteKit code and the same SQL. In Node and
Bun, `platform.invoke` is an async function that calls the prepared statement.

Each server also has a mode `hello`: a fixed body. For Embed, this is a
worker that returns `new Response('hello')`. Mode `axum` is Axum alone, with
no worker.

To run it:

```sh
node bench/seed.mjs
(cd bench/app && bun install && bun run build)
cargo build --release --features axum --example bench_server
bench/compare.sh 10 3
```

`bench/compare.sh` pins the server to CPUs 0 and 1, and oha to CPUs 2 and 3.
For each case it runs oha for 3 s to warm the server. Then, three times, it
runs 10 s at 1 connection and 10 s at 64 connections.

## Baseline

Commit of the bench, before any change. Each line is one run. p50 and p99 are
the latency at 1 connection. req/s is the throughput at 64 connections.

| server | path         | p50, 1 conn (ms)    | req/s, 64 conn         |
| ------ | ------------ | ------------------- | ---------------------- |
| Embed  | `/`          | 1.175, 1.180, 1.165 | 1223, 1515, 1460       |
| Node   | `/`          | 0.747, 0.641, 0.657 | 1468, 1499, 1474       |
| Bun    | `/`          | 0.277, 0.247, 0.256 | 4165, 4345, 4349       |
| Embed  | `/api/stats` | 0.942, 0.885, 0.860 | 2841, 2875, 2564       |
| Node   | `/api/stats` | 0.256, 0.242, 0.264 | 3780, 4053, 4020       |
| Bun    | `/api/stats` | 0.099, 0.100, 0.097 | 12675, 12513, 12246    |
| Embed  | hello        | 0.176, 0.185, 0.179 | 15976, 14746, 15308    |
| Node   | hello        | 0.047, 0.048, 0.048 | 40047, 37601, 37559    |
| Bun    | hello        | 0.041, 0.041, 0.040 | 107459, 112457, 119320 |
| Axum   | hello        | 0.043, 0.043, 0.044 | 176349, 186786, 184761 |

What the baseline shows:

- Axum alone answers in 0.043 ms. Node answers in 0.047 ms. So the HTTP
  layer of Embed costs no more than the HTTP layer of Node.
- A worker that returns a fixed body adds about 135 microseconds to Axum
  (0.179 ms against 0.043 ms). On the M5 Max of CLAUDE.md, the same gap is
  about 34 microseconds. So a cost of this VM is much larger than on macOS.
- The three reads with no render cost about 700 microseconds in Embed
  (`/api/stats` 0.89 ms against hello 0.18 ms). They cost about 200
  microseconds in Node (0.25 ms against 0.047 ms).
- The render costs about 290 microseconds in Embed (`/` 1.17 ms against
  `/api/stats` 0.89 ms) and about 400 microseconds in Node (0.68 ms against
  0.25 ms). The render is not the problem.
