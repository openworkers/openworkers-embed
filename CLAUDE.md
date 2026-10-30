# openworkers-embed - branch experiment/warm-places

This branch is an experiment. Do not merge it. It exists to find why one worker
thread of Embed costs more per request than Node.js and Bun.

## Problem

Embed renders a SvelteKit page with V8 inside a Rust process. On one worker
thread it is slower than Node.js and Bun on the same page.

Page `/` of the rock-paper-scissors app (SSR, 3 SQLite reads), Apple M5 Max,
release build, `oha`, one server process each:

| server             | median, 1 connection | requests per second, 64 connections |
| ------------------ | -------------------- | ----------------------------------- |
| Bun (`bun:sqlite`) | 0.10 ms              | 15 250                              |
| Node (`node:sqlite`) | 0.14 ms            | 9 100 to 9 400                      |
| Embed, 1 thread    | 0.18 ms              | 6 470 to 7 200                      |

The JSON route (no render) takes 0.139 ms in Embed and 0.063 ms in Bun.
A server that answers a fixed body takes 0.087 ms in Embed and 0.053 ms in Node.
The render alone takes 0.097 ms in Embed and 0.092 ms in Node. So the cost is
outside the render.

Other measures:

- One `platform.invoke` costs 26 to 32 microseconds.
- A real snapshot does not change the steady state (0.184 ms and 0.182 ms).
- In-process times with no HTTP (criterion, `benches/fetch.rs`): a minimal
  worker 18 microseconds, an `invoke` 21 microseconds, the SSR of the example
  117 microseconds. Run them again before you use them.

## Questions to answer

Find where the time goes for one request at one thread. Rank the costs in
microseconds, with evidence. Candidates:

- The HTTP layer: Axum, hyper, and the conversion of the request and response.
- The hop to the worker thread: channel, `oneshot`, `spawn_local`.
- `reset()` of the context before each request (it runs JavaScript).
- The conversion of `Event` and `Response` between Rust and JavaScript.
- The JSON round trip of each `platform.invoke`.
- The global `Mutex<HashMap>` of foreground tasks in `openworkers-runtime-v8`
  (`src/platform.rs`).
- The `select` and `spawn_local` of `respond()` in `src/engine/mod.rs`.

## What the branch holds

- `src/engine/warm.rs`: a V8 engine with one isolate and one context per
  place. It replaces `execute_pinned` and uses only the public API of the
  runtime. A place costs about 50 MB.
- `INV_PLACES` (places per thread, default 8) is a temporary switch.
- `src/pool.rs` sends a request to thread 0 when every thread is idle. This
  gives a median of 0.17 ms at one connection and lowers the throughput at 64
  connections. It is a temporary rule.
- The numbers of the variants are in the commit message.

`main` holds the pool of `openworkers-runtime-v8` with 8 slots per thread and a
semaphore of 8 permits.

## How to measure

- Use one thread: `Embed::builder(code).threads(1)`.
- Profile with `sample` (macOS) or `samply` while `oha -z 20s -c 64` runs.
- Time the layers one by one: a worker that returns a fixed body, a worker that
  calls one command, then the SSR bundle.
- Compare with Node.js on the same page, with the same tool and flags.
- Run each case at least three times. Runs vary by 10 to 20 percent.
- `cargo bench --bench fetch` runs the in-process cases. The SSR cases need the
  build of `examples/sveltekit-app`.

## Rules

- Commit on this branch only. Never push.
- One change per commit. Put the numbers in the body of the commit message.
- Write each number with its unit and what it measures.
- Never commit an image file.
- Use ASCII only in code, comments and documents.
- Write comments and documents in simple English: one idea per sentence.
- Run `cargo fmt` and `cargo clippy --all-targets` before each commit.
- Run `cargo test` before each commit. All tests must pass.

## Deliverable

A ranked list of the costs per request at one thread, in microseconds, with the
profile or the measure that shows each one. For each cost, a fix and its
measured effect on the median at one connection and on the requests per second
at 64 connections.
