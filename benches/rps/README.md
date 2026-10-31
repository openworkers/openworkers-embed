# Rock-paper-scissors bench

This bench compares Embed, Node.js and Bun on page `/` of a real SvelteKit
app. The same `_worker.js` runs on the three hosts. Each host owns SQLite and
answers `platform.invoke('query')`.

## The app

- Repository: https://github.com/max-lt/rock-paper-scissors, commit cf58db8.
- `app.patch` sets `@openworkers/adapter-sveltekit` to 0.6.2 and changes
  `src/hooks.server.ts`. The hook gives the app a `DATABASE` that calls
  `platform.invoke('query', { sql, params })`. It rewrites the Postgres SQL of
  the app for SQLite, once per statement.
- Page `/` with the cookie of the bench player does 3 reads: the player, the
  leaderboard (10 rows) and the open game. It writes nothing.

```sh
git clone https://github.com/max-lt/rock-paper-scissors app
cd app && git checkout cf58db8 && git apply ../app.patch
bun install && bun run build
sqlite3 ../rps.sqlite < seed.sql
```

## The hosts

| Host  | File               | HTTP          | SQLite                  |
| ----- | ------------------ | ------------- | ----------------------- |
| Embed | `examples/rps.rs`  | Axum          | rusqlite, bundled       |
| Node  | `benches/rps/node.mjs` | `node:http` | `node:sqlite`         |
| Bun   | `benches/rps/bun.ts`   | `Bun.serve` | `bun:sqlite`          |

Embed uses 1 worker thread unless `THREADS` is set. Node and Bun use one
thread for JavaScript.

## How to run

```sh
cargo build --release --features axum --example rps --example ladder
benches/rps/run.sh <app>/build <rps.sqlite> 5 10
benches/rps/ladder.sh <app>/build/assets 5 5
```

- `run.sh` measures page `/` on the three hosts.
- `ladder.sh` measures the layers one by one (see below).
- Each round starts each server, warms it for 2 or 3 s at 64 connections, then
  measures 1 connection and 64 connections with `oha`.
- The servers take turns in each round, so a change of the machine load falls
  on all of them.
- Each line gives: case, round, connections, requests per second, p50 in ms,
  p99 in ms, load average over 1 minute.
- The raw lines are in `results/`.

## The ladder

Each case adds one layer to the case before it.

| Case             | What runs                                                   |
| ---------------- | ----------------------------------------------------------- |
| `e-axum`         | Axum answers "hello".                                       |
| `e-hop`          | Axum sends a job to a thread as `src/pool.rs` does, and waits. |
| `e-hello`        | Embed, 1 thread, `ladder/hello.js`.                         |
| `e-hello-assets` | The same, with an assets directory.                         |
| `e-stream`       | Embed, the body is a `ReadableStream`.                      |
| `e-invoke3`      | Embed, 3 calls of `env.EMBED.invoke`.                       |
| `n-raw`          | `node:http` answers "hello".                                |
| `n-hello`        | Node, `ladder/hello.js` behind `node:http`.                 |
| `n-stream`       | Node, the body is a `ReadableStream`.                       |
| `n-invoke3`      | Node, 3 calls of `env.EMBED.invoke`.                        |

## Machine

- Apple M2, 8 cores, macOS. This is not the Apple M5 Max of the numbers in
  `CLAUDE.md`, so compare the hosts, not the absolute numbers.
- A video call ran during the runs (about 25 percent of one core). The load
  average of each run is in the raw lines.

## Results

Values are min / median / max over 5 runs. "1 conn" is 1 connection, "64 conn"
is 64 connections.

### 00 - Baseline, page /, commit of this file

| Host  | req/s, 1 conn      | p50, 1 conn (ms)      | p99, 1 conn (ms)   | req/s, 64 conn     |
| ----- | ------------------ | --------------------- | ------------------ | ------------------ |
| Embed | 2172 / 2224 / 2233 | 0.430 / 0.432 / 0.438 | 0.95 / 0.98 / 1.05 | 2818 / 3126 / 3273 |
| Node  | 2812 / 2844 / 2853 | 0.339 / 0.340 / 0.343 | 0.41 / 0.42 / 0.44 | 3802 / 3810 / 3830 |
| Bun   | 4571 / 4619 / 4680 | 0.206 / 0.207 / 0.210 | 0.41 / 0.42 / 0.44 | 5406 / 5489 / 5662 |

- The gap to Node is 92 microseconds on the p50 at 1 connection.
- At 64 connections the thread is busy all the time. Embed spends 320
  microseconds per request, Node 262. The gap is 58 microseconds of work.
- The p99 of Embed at 1 connection is 2.3 times the p99 of Node.
- Load average 2.0 to 2.9 during the runs.
