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

## The hop to the worker thread

Mode `hop` of `examples/bench_server.rs` sends each request to another thread
the way `src/pool.rs` does: an unbounded channel, `spawn_local`, and a
`oneshot` back. It runs no JavaScript.

| server     | p50, 1 conn (ms)    | req/s, 64 conn        |
| ---------- | ------------------- | --------------------- |
| Axum       | 0.044, 0.044, 0.044 | 176165, 178820, 183938 |
| Axum + hop | 0.088, 0.088, 0.088 | 98717, 97742, 87918    |

The hop costs 44 microseconds at 1 connection. At 1 connection both threads
sleep between requests. Each request wakes the worker thread, then the Axum
thread. strace counts 2 futex, 2 write and 3 epoll_wait per request in mode
hello, against 1 epoll_wait for Axum alone. Node and Bun serve HTTP and run
JavaScript on one thread, so they pay no hop.

## The CPU limit on Linux

`warm.rs` keeps the default CPU limit of 50 ms. On Linux, the runtime makes
a POSIX timer for each request. It arms and disarms the timer at each turn
of the event loop. It reads the CPU time of the thread at each turn. strace
counts per request in mode hello: 1 timer_create, 1 timer_delete, 10
timer_settime and 11 clock_gettime. Outside Linux there is no timer.

With `max_cpu_time_ms: 0`, these syscalls go:

| case         | p50 before (ms)     | p50 after (ms)      | req/s before        | req/s after         |
| ------------ | ------------------- | ------------------- | ------------------- | ------------------- |
| hello        | 0.176, 0.185, 0.179 | 0.158, 0.166, 0.174 | 15976, 14746, 15308 | 22590, 21647, 20206 |
| `/api/stats` | 0.942, 0.885, 0.860 | 0.819, 0.880, 0.851 | 2841, 2875, 2564    | 2914, 3097, 2801    |
| `/`          | 1.175, 1.180, 1.165 | 1.118, 1.136, 1.177 | 1223, 1515, 1460    | 1666, 1568, 1644    |

The limit costs about 20 microseconds of CPU per request on this VM
(15300 against 21500 req/s at 64 connections on hello). It is not kept off:
the test `a_worker_the_runtime_stops_ends_its_stream_with_an_error` needs
it. The fix belongs in the runtime: one timer per thread instead of one per
request, and fewer syscalls per turn.
