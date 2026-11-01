# Rock-paper-scissors bench

This bench compares Embed, Node.js and Bun on page `/` of a real SvelteKit
app. The same `_worker.js` runs on the three hosts. Each host owns SQLite and
answers `platform.invoke('query')`.

## Summary of the night of 2026-10-31

At one worker thread on page /, Embed now serves 25 percent more requests
per second than at the start and 4.4 percent more than Node.js (ranges
apart); its p50 at 1 connection dropped by 96 us and is equal to Node's
within the spread (median 3 us lower; the ranges overlap by 3 us). One
worker thread runs the JavaScript, but Embed parses HTTP on other threads
(Axum on tokio), while Node and Bun do everything on one thread; see
"One thread" below.
Bun stays ahead (see below); the work on Bun stops here, and JSC stays a
supported engine of Embed. With 2 worker threads, Embed serves 3.4 percent
more requests per second than Node.js with 2 cluster processes (ranges
apart); with 4, the two are equal within the spread. See "21 - Several
threads".

### Start and end, under the same conditions

Page / of rock-paper-scissors, Apple M2, release, oha, 5 runs of 10 s, the
four servers in turn in each round, min / median / max. Start: embed afea4f8
(the code of 3acc67b with the bench) and openworkers-runtime-v8 v0.18.5. End:
embed at the end of the night with openworkers-runtime-v8 at the end of
experiment/embed-perf (through a [patch] with a path).

| Host  | p50, 1 conn (us) | p99, 1 conn (us)  | req/s, 64 conn     |
| ----- | ---------------- | ----------------- | ------------------ |
| start | 432 / 435 / 441  | 965 / 978 / 1063  | 3045 / 3154 / 3210 |
| end   | 337 / 339 / 342  | 849 / 863 / 903   | 3886 / 3948 / 3989 |
| node  | 339 / 342 / 349  | 411 / 417 / 500   | 3751 / 3782 / 3798 |
| bun   | 207 / 207 / 211  | 416 / 418 / 442   | 5514 / 5596 / 5607 |

- The p50 at 1 connection drops by 96 us (22 percent); the req/s at 64
  connections rise by 25 percent.
- Against Node: the p50 is equal within the spread (median 3 us lower; the
  ranges overlap by 3 us, 337 to 342 against 339 to 349; the runs of steps
  07 to 12 had them apart). The req/s at 64 connections are 4.4 percent
  higher, ranges apart, with HTTP on other threads (see below).
- The p99 at 1 connection is still about twice Node's (see the open items).

Stream of 1000 pieces of 1 byte, p50 at 1 connection, 3 runs:

| Host  | p50, 1000 pieces |
| ----- | ---------------- |
| start | 147.7 to 149.1 ms |
| end   | 12.9 to 13.1 ms  |
| node  | 13.3 ms          |
| bun   | 0.2 ms           |

### What changed, in order, with its gain

p50 at 1 connection and req/s at 64 connections on page /, each step against
the build before it, 5 runs of 10 s.

| Step | Commit | What | p50, 1 conn | req/s, 64 conn |
| ---- | ------ | ---- | ----------- | -------------- |
| 01 | embed c66013d | Assets indexed at start: no blocking-pool hop for `/` | 434 -> 390 us | in spread |
| 02 | embed c4cca79 | Each request is a task of the runtime, not of a LocalSet: an op round trip 12.2 us -> 0.8 us | 387 -> 350 us | in spread |
| 03 | embed 171c2bf | A new request waits while another one runs on its thread (cache of the isolates) | 349 -> 343 us | 3236 -> 3856 |
| 07 | runtime-v8 5715f34, df27bac, 77855ff; embed 7b1c0e5 | Inline invoke: a ready host function settles in the call | 343 -> 333 us | 3861 -> 3992 |
| 10 | runtime-v8 ffe9925 | reset() calls a function compiled once per context | in spread | 3983 -> 4011 |
| 12 | runtime-v8 01b5cb7 | The body pump waits for room instead of a 1 ms timer: 1000 pieces 161 ms -> 12.9 ms | no change | no change |

In process (criterion), one invoke cost 18 us at the start and costs about 2.5
us at the end; a hello worker went from 21.7 us to 19.6 us.

### One thread

Embed with Axum runs JavaScript on one worker thread and HTTP on the tokio
threads, in parallel. Per-thread CPU during a hello load at 64 connections
(`ps -M`): the worker thread 94.5 percent, several tokio threads about 27
percent each. Node and Bun parse HTTP and run JavaScript on the same thread.
With serve() (experiment/serve, 7bddd55), Embed does everything on the worker
thread (98.6 percent, the others idle): a hello worker at 64 connections
serves 41.6k req/s against 52.7k with Axum (Node 45.5k in table 00, another
run); on page / serve and Axum are equal within the spread (3780 against 3733
req/s, Node 3794 in the same runs).

### Safety and correctness fixes

- macOS CPU limit: a turn that never yields was never cut on macOS (no
  answer after 15 s). A watchdog thread now cuts it in 53 to 62 ms
  (runtime-v8 6e4062a, d42f887). No measured cost.
- Top level of a script (from before tonight, review 7): it ran with no CPU
  guard and no wall clock on every platform, so `while (true) {}` at top
  level pinned a thread for ever; Linux too, from the code. It now runs as
  a turn under both, and fails in 63 to 69 ms (runtime-v8 2bfa399).
- reset() (from before tonight, review 8): it called JavaScript that the
  guest could reach (Map.prototype.clear of the timer maps) outside any
  guard, so a guest that replaced it with a loop hung the next request. The
  clear function is now made before guest code, with the builtins it
  calls, the timer map globals are read-only, and reset runs under the CPU
  limit and the wall clock (runtime-v8 48ab1c9).
- Both guards of the top level and of reset() are in
  openworkers-runtime-v8 v0.18.6. There the top level gets its own CPU
  floor: 400 ms, or the request limit if larger.
- Streams: a cancel closes a stream whose pump waits for room (runtime-v8
  d6c230d).
- Inline invoke: panics of a host function reject the promise (they hung
  before); a loop of ready calls gives the thread back after 16 calls per
  turn; the time of a sync command counts in the CPU limit (stricter than
  before).
- Assets: names that start with a dot are never served except
  /.well-known (4536a4d); the asset path comes from the request line, not
  the Host header (ffb3b63); rebuilds, links, aliases, FIFOs, // and /./
  (073b682, 8b244ad, a21abd1, b131550).
- Admission: a request whose caller is gone does not start (77b64db,
  c903079, cc10413, 3967d85); MAX_HOLD bounds a wait to 10 ms; FIFO starts.
- INV_PLACES: a bad, non UTF-8 or changed value fails Embed::build
  (dd89f6a, 16c0b39, d5e1719).
- A panicking job gives EmbedError::Panicked (4abc89c); a relay that
  panics cuts the body with an error (083bfac, d045e46).
- JSC: a CPU budget per request, as on V8 (runtime-jsc d912f92): a chain of
  entries ran to the 30 s wall clock before; the timers and tasks of an
  event stop when it ends (0cea8bb); every call into JavaScript is under
  the budget, checked against a deadline (a243960). See Warm JSC engine.
- Nine review rounds; their findings are fixed in the commits above, except
  the known limits written here.

### Measured and not done

- serve(): HTTP on the worker thread, with no hop. p50 341 vs 353 us with
  Axum, but the ranges overlapped; branch experiment/serve (7bddd55).
- A lazy Request in openworkers-wintertc: at most about 1.9 us per request
  (URL parse 1.4 us, Headers 0.45 us), and the change is visible to
  workers (own properties become accessors). Not done.
- Internalized init keys (b424a9f): no gain in an A/B; reverted (8d90bc7).
- --no-single-threaded-gc (OW_V8_FLAGS, no code), on the end build, medians
  of 5 runs of 10 s (one round disturbed): p50 at 1 conn 336 -> 329 us,
  p99 825 -> 555 us, req/s at 64 conn 4002 -> 4212. runtime-v8 sets the
  flag on macOS on purpose (workers-sdk issue 2386); not changed.
- Bun: the engine is about 42 us of the 130 us gap (the same SSR in process:
  V8 94-100 us, JSC as shipped by Bun 51-59 us). Bun ships its own, newer
  JSC; Embed uses the system JavaScriptCore through runtime-jsc.
- Load cost of the rps bundle (parse and top level), for the CPU budget of
  a cold start: the first request takes 14.0 to 14.6 ms against 1.2 to 1.6
  ms warm (runs 2 to 5; 25 ms on the first run), so a place costs about 13
  ms. The runner gives 100 ms of CPU.

### Warm JSC engine

The jsc engine of Embed now keeps warm runtime-jsc workers per thread (at
most 8), resets them between requests and runs them under a CPU budget
per request (embed 563dee1, 51149d6; runtime-jsc bc6dfd5, aa8a593, 842a426,
d912f92, 0cea8bb, a243960, 02cba5f, a11109e, 41ae341, c4cc3e7, 733a5a5 on the
local branch experiment/warm-pool,
from v0.18.1).

- CPU limit: runtime-jsc had none (the wall clock is checked only between
  callbacks). It now arms JSContextGroupSetExecutionTimeLimit (private API
  of JavaScriptCore, declared with extern "C") before the top level of the
  script. A loop that never yields is cut after 51 ms (limit 50 ms); 240 ms
  of waiting with no CPU in one entry is not cut, so it counts CPU time. Fail
  closed: if a test loop is not cut at start, no worker starts. The top
  level gets the budget of a request or 400 ms, the larger (02cba5f), as on
  V8 (LOAD_CPU_TIME_MS, 388b40c on main, cbe7539 on experiment/embed-perf):
  a bundle that needs 100 ms to start makes a worker; one that needs 500 ms
  does not. The text of a thrown value is read even when logging is off
  (a11109e), and Worker::evaluate gives text, not values of the guest
  (41ae341).
- CPU budget per request (d912f92): before, the limit applied to each entry
  into JavaScript, so a request that chained entries of 40 ms with
  setTimeout(0) ran to the wall clock, 30 s with 29 s of CPU (V8: 56 ms).
  Now each entry arms the watchdog with what is left of the budget, and the
  CPU time of the thread is taken from it after the entry, as CpuEnforcer of
  runtime-v8: the chain ends with CpuTimeLimit after 53 ms, and a cut in the
  microtasks of an entry ends the request at once (52 ms, not at the wall
  clock).
- Every call into JavaScript (a243960): JavaScriptCore starts the watchdog
  again at each entry into the VM, and one API call can enter it twice:
  when an exception leaves JSEvaluateScript, its inspector reads the text
  of the value, which runs the toString of the guest in a new entry. Before,
  45 ms in a callback and then a thrown value whose toString never ends ran
  about 99 ms against a budget of 50 ms. Now each call from Rust goes
  through one helper that sets a deadline in thread CPU time; the watchdog
  fires every 5 ms of an entry at most, and its callback cuts the entry
  past the deadline: the same case ends under 80 ms. The host calls only
  functions made before guest code and read-only (the fetch and task
  triggers, the callback registry, the Response, TypeError and chunks of
  callbacks), so a guest that replaces Request or __runCallback changes
  nothing; before, the request timed out at the wall clock.
- Known limit: the watchdog does not cut native code. A backtracking
  regular expression, (a+)+b, ran 1.7 s against 50 ms; its time is taken
  from the budget when it returns.
- Wall clock: jsc_warm uses the default limits of the runtime, so a request,
  and a stream, stop at 30 s. A worker keeps its globals between requests,
  as on V8.
- Reset: timers, intervals, tasks and callbacks of an event do not reach the
  next one; the one call into JavaScript is made before guest code, holds
  its builtins, and runs under the limit.
- End of an event (0cea8bb): its timers, fetches, invokes and stream reads
  stop when it ends, so an idle worker queues no callback (an interval of
  1 ms queued 1998 callbacks in 2 s before). Its streams are not closed.
- JIT: the system JavaScriptCore runs without its JIT unless the binary has
  the com.apple.security.cs.allow-jit entitlement; with it, the CPU limit
  needs polling traps, which Embed turns on when it is built.

Page /, 3 runs of 5 s, p50 at 1 conn / req/s at 64 conn:

| Engine | p50, 1 conn | req/s, 64 conn |
| ------ | ----------- | -------------- |
| jsc, a worker per request (before; no result file) | 13.5 ms | 80 |
| jsc, warm, no JIT | 2091 us | 496 |
| jsc, warm, JIT and polling traps | 382 us | 2962 |
| v8 | 333 us | 3964 |
| node | 346 us | 3831 |

The warm JSC engine is about 36 times faster than before, but it stays
behind V8 with the system JavaScriptCore.

The budget per request costs nothing measurable. JIT and polling traps,
before (842a426) and after (d912f92, 0cea8bb), 2 rounds of 3 runs of 5 s
in turn, min / median / max (`results/20-jsc-budget.txt`):

| Build | p50, 1 conn (us) | req/s, 64 conn |
| ----- | ---------------- | -------------- |
| before | 377 / 382 / 386 | 2930 / 2979 / 2999 |
| budget | 382 / 383 / 385 | 2881 / 2930 / 2943 |

The ranges overlap at 64 connections (1.6 percent lower on the median).

The checks against a deadline (a243960) cost about 5 percent of requests per
second at 64 connections, with the same p50. JIT and polling traps, before
(0cea8bb) and after (a243960), built with the same features, 5 runs of 10 s
in turn, min / median / max (`results/22-jsc-deadline.txt`):

| Build | p50, 1 conn (us) | req/s, 64 conn |
| ----- | ---------------- | -------------- |
| before | 387 / 388 / 392 | 2777 / 2828 / 2847 |
| after | 386 / 389 / 391 | 2622 / 2692 / 2737 |

Two other runs of 3 (one with a period of 50 ms instead of 5 ms) gave a
median 4.6 and 4.8 percent lower. In process, without HTTP, the CPU time
per request of the two builds was the same within the spread. The cause
is not found.

### openworkers-jsc (experiment/jsc-e2e)

runtime-jsc on openworkers-jsc instead of rusty_jsc (runtime-jsc
experiment/openworkers-jsc, openworkers-jsc experiment/held-lock), in four
builds: the system framework or the static oven-sh build, with a VM lock
per C API call or a lock held for the life of the VM (held-lock, the model
of Bun). Base: runtime-jsc 02cba5f on rusty_jsc. All with the JIT and
polling traps; the system builds are ad-hoc signed with allow-jit. Page /,
jsc engine, one worker thread, 3 rounds of 10 s with the five servers in
turn, no build running, load 2.1 to 5.3. Min / median / max
(`results/23-openworkers-jsc.txt`):

| Build | p50, 1 conn (us) | p99, 1 conn (us) | req/s, 64 conn |
| ----- | ---------------- | ---------------- | -------------- |
| base | 369 / 373 / 375 | 710 / 711 / 723 | 2963 / 2978 / 2987 |
| system | 374 / 377 / 379 | 697 / 705 / 717 | 2935 / 2968 / 3003 |
| system, held-lock | 375 / 379 / 380 | 564 / 572 / 582 | 3183 / 3184 / 3226 |
| static | 367 / 370 / 370 | 606 / 607 / 609 | 2983 / 2985 / 3019 |
| static, held-lock | 369 / 371 / 371 | 838 / 854 / 861 | 3149 / 3152 / 3185 |

- The switch from rusty_jsc costs nothing measurable (system against base).
- held-lock gives 6 to 7 percent more req/s at 64 conn on both builds,
  with the same p50 at 1 conn.
- The RSS after the run at 64 conn: base 388 to 396 MB, system 394 to 402,
  system held-lock 346 to 402, static 377 to 393, static held-lock 308 to
  326.

### Stress test without --single-threaded-gc (macOS)

runtime-v8 sets --single-threaded-gc on macOS, citing workers-sdk issue
2386, which names no V8 flag. Embed (1a4eb86 with
runtime-v8 48ab1c9), release, INV_PLACES=8, a supervisor that keeps the
exit status and the signals, 30 min plus a 5 min control:

| Phase | Load | Flags | Crash | RSS (MB) | Throughput, p99 |
| ----- | ---- | ----- | ----- | -------- | --------------- |
| a, 15 min | page /, 64 conn, 1 thread | --no-single-threaded-gc | none | 213 -> 221, flat from min 7 | 3931-4181 req/s; p99 17.9-19.2 ms, once 29 ms |
| b, 10 min | page, stream 1000, hello, new Function per request; 16 conn each, 4 threads | --no-single-threaded-gc | none | 1269 -> 2656, peak 2717, still growing slowly | flat per route |
| c, 5 min | as b | + --stress-compaction --stress-flush-code | none | 302 -> 316, flat | about 40 percent of b |
| control, 5 min | as b | default (single-threaded GC) | none | 1035 -> 1459, peak 1562, still growing | as b |

No crash, no signal, no crash report, no hang. The errors were the requests
in flight when each 60 s window of oha ended (the same count with the
default flag). Memory is the open question: at 5 min the mixed load holds
2353 MB without the flag against 1459 MB with it, and neither has a plateau.
A longer run (30 to 60 min, with and without the flag) should come before
any change of the flag.

### Branches

- openworkers-embed experiment/warm-places: all the embed commits above.
- openworkers-embed experiment/serve: serve() (7bddd55).
- openworkers-runtime-v8 experiment/embed-perf: 5715f34 to cb5fd7b, from
  origin/main b847136.
- openworkers-runtime-v8 fix/top-level-cpu-guard: a325414, 23233bb, from
  v0.18.5.
- openworkers-runtime-jsc experiment/warm-pool: bc6dfd5, aa8a593, 842a426,
  d912f92, 0cea8bb, a243960, 02cba5f, a11109e, 41ae341, c4cc3e7, 733a5a5,
  from v0.18.1.

### Open items

- The p99 at 1 connection is about twice Node's: GC on the JS thread
  (--single-threaded-gc, set on macOS on purpose). See the A/B and the
  stress test above: no crash without the flag, but more memory, with no
  plateau seen yet.
- openworkers-runtime-jsc v0.18.1 has no CPU limit at all, and its wall
  clock does not cut a synchronous loop; a runner built with its "jsc"
  feature has this gap until it takes a release with the CPU budget.
- runtime-v8: a request that makes its context gets the budget of the top
  level and then a whole one of its own.
- Embed on V8 has no wall clock limit (max_wall_clock_time_ms: 0); on JSC
  it has the default of 30 s.
- Boa: nothing cuts a running script. A loop that never ends, with or
  without native calls, at the top level, in a request or after an await,
  and a deep recursion, all ran past 4 s with a CPU limit of 100 ms and a
  wall clock of 1 s. The wall clock ends a request only between two jobs.
  Boa has caps on loop iterations per call and on recursion depth, which
  runtime-boa does not set; they would not cut a callback loop in native
  code or a deep tree of calls.
- JSC: native code (a backtracking regular expression) is not cut by the
  watchdog; the streams of an event are not closed when it ends.
- Nova: `String.prototype.replace` substitutes nothing in a replacement
  template that does not start with `$` (section 25), so the build of the
  app answers 500 on page /. Nova does not cut a running script either.
- JSC: the budget counts the CPU time of the worker thread only. The GC
  and JIT threads of JavaScriptCore are not counted.
- JSC: an entry can run up to 5 ms of CPU past the budget before the
  watchdog cuts it (one period of its checks).
- JSC: a guest that replaces the helpers of a response (__finishFetch,
  __streamResponseBody) or the builtins they call (Promise.prototype.then,
  JSON.stringify) breaks only its own response, under the CPU budget and
  the wall clock.
- JSC: the checks against a deadline cost about 5 percent of requests per
  second on page / at 64 connections; the cause is not found.
- openworkers-core builds the URL of a request from the Host header without
  a check (embed works around it).
- clippy --all-targets fails on two examples of runtime-v8, as on
  origin/main.

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

Embed uses 1 worker thread for JavaScript unless `THREADS` is set; Axum
parses HTTP on the tokio threads. Node and Bun do HTTP and JavaScript on one
thread.

For several threads, `benches/rps/node-cluster.mjs` runs `node.mjs` in
`WORKERS` processes behind `node:cluster`, and `benches/rps/bun-n.sh` runs
`bun.ts` in `WORKERS` processes on one port with `reusePort`.

## How to run

```sh
cargo build --release --features axum --example rps --example ladder
cargo run --release --bin snapshot
benches/rps/run.sh <app>/build <rps.sqlite> 5 10
benches/rps/ladder.sh <app>/build/assets 5 5
```

- Cargo.toml takes openworkers-runtime-v8 and openworkers-runtime-jsc from
  the sibling checkouts ([patch]). Cargo only warns when a Cargo.lock pins
  another version of them, and the build then runs without their fixes.
  `cargo test --test patched` fails in that case; `cargo update -p
  openworkers-runtime-v8` (or `-p openworkers-runtime-jsc`) fixes it.
- `run.sh` measures page `/` on the three hosts.
- `ladder.sh` measures the layers one by one (see below).
- `threads.py <app>/build <rps.sqlite>` measures 1, 2 and 4 threads or
  processes on the three hosts (see "21 - Several threads").
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

- Apple M2, 8 cores, macOS. Compare the hosts, not the absolute numbers.
- The 8 cores are 4 performance cores and 4 efficiency cores. With 4
  threads or more, some of the work may run on efficiency cores.
- The load average of each run is in the raw lines.

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

### 00 - Ladder, before any fix

5 runs of 5 s. p50 and p99 at 1 connection in microseconds, req/s at 64
connections. Load average 3.3 to 8.6.

| Case             | p50, 1 conn (us) | p99, 1 conn (us) | req/s, 64 conn         |
| ---------------- | ---------------- | ---------------- | ---------------------- |
| `e-axum`         | 38 / 38 / 39     | 53 / 56 / 59     | 116824 / 118291 / 120356 |
| `e-hop`          | 51 / 51 / 52     | 66 / 67 / 75     | 108335 / 108544 / 109013 |
| `e-hello`        | 87 / 87 / 87     | 108 / 110 / 132  | 51593 / 53331 / 53720  |
| `e-hello-assets` | 99 / 99 / 99     | 122 / 122 / 147  | 47703 / 48244 / 50369  |
| `e-stream`       | 87 / 87 / 87     | 107 / 109 / 112  | 45348 / 48144 / 49298  |
| `e-invoke3`      | 141 / 141 / 144  | 168 / 173 / 177  | 23891 / 25467 / 27748  |
| `n-raw`          | 44 / 45 / 45     | 56 / 59 / 62     | 89370 / 90910 / 91176  |
| `n-hello`        | 71 / 71 / 72     | 87 / 87 / 92     | 44722 / 45455 / 45857  |
| `n-stream`       | 67 / 67 / 69     | 83 / 86 / 88     | 49691 / 50683 / 51178  |
| `n-invoke3`      | 72 / 72 / 73     | 89 / 91 / 96     | 44433 / 44828 / 45295  |

What the ladder shows, on the p50 at 1 connection:

- Axum is 7 us faster than `node:http`. HTTP is not the gap.
- The hop to the worker thread costs 13 us.
- The hello worker costs 36 us after the hop in Embed. In Node, the worker
  and the conversion of the request and the response cost 26 us.
- The assets lookup before the worker costs 12 us.
- A `ReadableStream` body costs nothing more than a plain body.
- 3 calls of `invoke` cost 54 us in Embed (18 us each) and 1 us in Node.
- At 64 connections, the 3 calls cost 20.5 us of thread time (7 us each).
  So most of the 18 us at 1 connection is waiting, not work.

### 01 - Index the assets at start

`src/assets.rs` indexes the names of the files at start, as sirv does in
production. A path that names no file, such as `/`, gets no system call and
no hop to the blocking pool of tokio. 5 runs of 10 s, the build before the fix
(`base`) and after it (`embed`) in turn with Node, load average 1.7 to 4.4.

| Host  | p50, 1 conn (us) | p99, 1 conn (us)  | req/s, 64 conn     |
| ----- | ---------------- | ----------------- | ------------------ |
| base  | 427 / 434 / 437  | 960 / 967 / 1021  | 3000 / 3139 / 3214 |
| embed | 384 / 390 / 393  | 901 / 907 / 924   | 3100 / 3233 / 3325 |
| node  | 340 / 340 / 340  | 412 / 412 / 419   | 3790 / 3797 / 3803 |

- The p50 at 1 connection drops by 44 us. The ranges do not overlap.
- This is more than the 12 us of the ladder. Between two SSR requests the
  thread of the blocking pool goes to sleep, so waking it costs more.
- The req/s at 64 connections gain 3 percent, which is inside the spread.
- The gap to Node on the p50 is now 50 us.

### 02 - Run each request as a task of the runtime, not of a LocalSet

The runtime of V8 runs its scheduler and its ops with `tokio::spawn`. Embed
ran each request as a task of a `LocalSet`, which lives in the `block_on`
future of the worker thread. On a current-thread runtime, each wake between
the `block_on` future and a task of the runtime goes through the I/O driver.

`examples/hop.rs` (`cargo run --release --example hop`) measures the round
trip of an op without V8 (a message to a scheduler task, which spawns an op
task, which answers on a channel):

| Waiter                     | Ops            | Round trip (ns) |
| -------------------------- | -------------- | --------------- |
| task of a LocalSet         | `tokio::spawn` | 12356 to 12540  |
| `block_on` of a LocalSet   | `tokio::spawn` | 12221 to 12224  |
| `block_on` of the runtime  | `tokio::spawn` | 12212 to 12217  |
| task of the runtime        | `tokio::spawn` | 777 to 781      |
| task of a LocalSet         | `spawn_local`  | 794 to 804      |

`src/pool.rs` now runs each request as a task of the runtime of the worker
thread. In process (criterion, `benches/fetch.rs`), one `invoke` drops from
39.5 us to 24.2 us. A hello worker drops from 21.7 us to 21.3 us.

Page /, 5 runs of 10 s, the build of step 01 (`base`) and this build
(`embed`) in turn with Node, load average 1.9 to 5.9:

| Host  | p50, 1 conn (us) | p99, 1 conn (us) | req/s, 64 conn     |
| ----- | ---------------- | ---------------- | ------------------ |
| base  | 381 / 387 / 391  | 883 / 890 / 901  | 3199 / 3310 / 3312 |
| embed | 348 / 350 / 351  | 830 / 839 / 843  | 3246 / 3261 / 3270 |
| node  | 339 / 339 / 341  | 410 / 413 / 416  | 3790 / 3794 / 3805 |

- The p50 at 1 connection drops by 37 us. The gap to Node is now 11 us.
- The req/s at 64 connections do not change. Embed reaches 86 percent of
  Node. At 64 connections the thread is busy, so the wakes cost little; the
  gap there is work: 307 us per request against 264 us for Node.
- The p99 at 1 connection is still twice the p99 of Node.

### Where the gap at 64 connections is

- SQLite costs the same in Embed and Node. Measured inside the servers at 64
  connections: 33 us per query in Embed, 34.9 us in Node, 3 queries per
  request. In a loop, the 3 queries cost 81 us with rusqlite, 82 us with
  `node:sqlite` and 92 us with `bun:sqlite`.
- `MEMO=1` keeps the rows of each query after the first run, in the three
  hosts. Node and Bun then get the same objects back each time, which is the
  most favorable case for them. With it, Embed serves no more requests at 64
  connections than at 1 connection (about 4300 req/s), and Node serves 24
  percent more (6150 req/s).
- `INV_PLACES` on page /, 3 rounds of 5 s, req/s at 64 connections: 1 place
  3834 to 3852, 2 places 3563 to 3677, 8 places 3159 to 3241 (Node 3794).
  Each place is an isolate. Requests that take turns on one thread run on
  different isolates, and each turn finds the caches of the CPU cold.

### 03 - A new request waits while another one runs on its thread

`src/engine/warm.rs` starts a new request only when no other request runs
on the thread, or when the thread is about to park (every running request
waits for something outside the thread), or after 10 ms with no progress.
The number of places stays 8, so requests that wait for I/O still overlap.

5 runs of 10 s, the build of step 02 (`base`) and this build (`embed`) in
turn with Node, load average 1.9 to 5.1:

| Host  | p50, 1 conn (us) | p99, 1 conn (us) | req/s, 64 conn     |
| ----- | ---------------- | ---------------- | ------------------ |
| base  | 343 / 349 / 350  | 830 / 837 / 858  | 3073 / 3236 / 3253 |
| embed | 341 / 343 / 345  | 821 / 827 / 833  | 3845 / 3856 / 3886 |
| node  | 339 / 340 / 341  | 410 / 414 / 419  | 3807 / 3823 / 3837 |

- The req/s at 64 connections rise by 19 percent. Embed now serves more
  requests than Node; the ranges do not overlap.
- The p50 at 1 connection is 3 us above Node.
- The version without the 10 ms bound gave the same numbers
  (`results/03-admission-unbounded.txt`).

Wait of a request to / behind another request on the same thread, from the
tests `a_request_starts_while_another_one_*`:

| The other request                                  | No bound | 10 ms bound |
| -------------------------------------------------- | -------- | ----------- |
| streams for 2 s, a piece every 10 ms               | < 300 ms | < 300 ms    |
| calls a command that is always ready, for 1 s      | 77 ms    | 3 to 5 ms   |
| calls a command that works 1 ms in Rust, for 1 s   | 977 ms   | 22 to 24 ms |

In the second case the CPU limit (50 ms) ends the other request. It counts
the time of the JavaScript only, so it does not end the third one.

### 04 - Assets: rebuilds, links and odd paths (review of step 01)

- A path whose last segment has a dot is looked for on disk when the index
  misses it, so the chunks of a rebuild and a root made after start are
  served. Page routes such as `/` stay in memory.
- Links to directories are followed again, once per canonical path.
- `//` and `/./` fold before the lookup.

5 runs of 10 s, the build of step 03 (`base`) and this build (`embed`):

| Host  | p50, 1 conn (us) | p99, 1 conn (us) | req/s, 64 conn     |
| ----- | ---------------- | ---------------- | ------------------ |
| base  | 341 / 342 / 344  | 825 / 828 / 863  | 3839 / 3861 / 3865 |
| embed | 342 / 342 / 344  | 823 / 831 / 834  | 3864 / 3874 / 3883 |
| node  | 339 / 340 / 340  | 408 / 410 / 416  | 3806 / 3814 / 3859 |

No change on page /.

### 07 - Inline invoke (openworkers-runtime-v8, branch experiment/embed-perf)

`Cargo.toml` takes openworkers-runtime-v8 from `../openworkers-runtime-v8`
(commits 5715f34 and df27bac of the local branch experiment/embed-perf). A
host function that answers at once settles its promise inside the call, with
no trip through the scheduler and the event loop.

In process (criterion, `benches/fetch.rs`), before and after:

| Case     | Before (us) | After (us) |
| -------- | ----------- | ---------- |
| hello    | 21.26       | 21.57      |
| invoke   | 24.78       | 23.65      |
| invoke3  | 31.42       | 25.47      |

Page /, 5 runs of 10 s, the build of 3967d85 without the patch (`base`) and
with it (`embed`) in turn with Node, load average 2.0 to 3.6:

| Host  | p50, 1 conn (us) | p99, 1 conn (us) | req/s, 64 conn     |
| ----- | ---------------- | ---------------- | ------------------ |
| base  | 340 / 343 / 345  | 816 / 821 / 852  | 3856 / 3861 / 3874 |
| embed | 333 / 333 / 334  | 801 / 808 / 829  | 3903 / 3992 / 3998 |
| node  | 340 / 340 / 341  | 410 / 413 / 416  | 3809 / 3820 / 3828 |

- Embed is below Node on the p50 at 1 connection (7 us) and above it on
  the req/s at 64 connections (4.5 percent). The ranges do not overlap.
  Goal 1 is met.
- The p99 at 1 connection is still about twice the p99 of Node.

### 09 - CPU limit on macOS (openworkers-runtime-v8 6e4062a)

Before, a worker with `while (true) {}` never ended on macOS (no answer
after 15 s, 3 runs): the CPU limit was only taken at the end of a turn. A
watchdog thread of openworkers-runtime-v8 now cuts such a turn after 53 to 62
ms (budget 50 ms, one tick of 5 ms).

Criterion without -> with the watchdog: hello 21.68 -> 21.53 us, invoke
23.00 -> 23.58 us, invoke3 25.20 -> 25.80 us.

Page /, 5 runs of 10 s, the build without the watchdog (`base`) and with it
(`embed`) in turn with Node, load average 2.0 to 5.3:

| Host  | p50, 1 conn (us) | p99, 1 conn (us) | req/s, 64 conn     |
| ----- | ---------------- | ---------------- | ------------------ |
| base  | 334 / 335 / 338  | 814 / 823 / 879  | 3980 / 3987 / 3996 |
| embed | 334 / 336 / 337  | 822 / 830 / 855  | 3608 / 3975 / 3978 |
| node  | 340 / 341 / 342  | 413 / 414 / 426  | 3801 / 3806 / 3814 |

No cost above the spread (the 3608 run had a load average of 5.3).

### 12 - The cost of a streamed piece (closed by runtime-v8 01b5cb7)

A worker that streams 1000 pieces of 1 byte, through HTTP at 1 connection
(p50, 3 runs; the same worker with 1 piece takes 88 to 89 us in Embed and 66
us in Node):

| Host  | 1000 pieces (ms)  | per piece (us) |
| ----- | ----------------- | -------------- |
| Embed | 160.7 to 161.6    | about 161      |
| Node  | 13.1              | about 13       |

In process, a piece costs about 10 us of CPU in Embed: 5000 pieces sat at the
edge of the CPU limit (50 ms) in `a_long_stream_arrives_whole`. Over HTTP,
the rest is waiting: each piece seems to go through a full round trip (the
JavaScript stream, the stream manager, the relay, hyper and the socket)
before the next pull. Page / sends its body in one piece, so it does not pay
this.

The body pump of runtime-v8 waited with a 1 ms timer when the channel of the
stream was full (16 pieces). It now waits for room (runtime-v8 01b5cb7). A
stream of 1 and 1000 pieces of 1 byte, p50 at 1 connection, 3 runs:

| Case        | Before         | After          | Node           |
| ----------- | -------------- | -------------- | -------------- |
| 1 piece     | 87 to 88 us    | 86 to 87 us    | 67 to 70 us    |
| 1000 pieces | 158 to 162 ms  | 12.8 to 12.9 ms | 13.08 to 13.10 ms |

Page /, 5 runs of 10 s, the build before (`base`) and after (`embed`):

| Host  | p50, 1 conn (us) | p99, 1 conn (us) | req/s, 64 conn     |
| ----- | ---------------- | ---------------- | ------------------ |
| base  | 331 / 331 / 332  | 803 / 810 / 812  | 4027 / 4033 / 4040 |
| embed | 332 / 332 / 333  | 801 / 803 / 810  | 3997 / 4022 / 4032 |
| node  | 340 / 340 / 342  | 410 / 411 / 417  | 3820 / 3823 / 3837 |

No change on the page, which sends one piece.

### 21 - Several threads

Page /, `threads.py`, 3 rounds of 10 s, each after a warm-up of 3 s at 64
connections. Embed runs V8 with `THREADS` worker threads (the engine of
this branch). Node runs `node:cluster` with as many processes. Bun runs as
many processes with `reusePort`. No build tool ran during a measure (the
script waits and measures again). Load average 2.6 to 5.3. Raw lines:
`results/21-threads.txt`.

"n" is the number of worker threads (Embed) or processes (Node, Bun). CPU is
the CPU time of the server and its children over the time of the measure,
from `ps`: 100 is one core.

64 connections:

| Host  | n | req/s                  | p50 (ms)       | p99 (ms)          | CPU (%)         |
| ----- | - | ---------------------- | -------------- | ----------------- | --------------- |
| embed | 1 | 3910 / 3923 / 3940     | 16.1 / 16.2 / 16.2 | 19.5 / 19.7 / 20.2 | 122 / 123 / 123 |
| node  | 1 | 3752 / 3760 / 3775     | 16.2 / 16.2 / 16.3 | 32.4 / 32.5 / 32.5 | 112 / 113 / 113 |
| bun   | 1 | 5517 / 5561 / 5620     | 11.2 / 11.3 / 11.4 | 22.3 / 22.5 / 22.6 | 101 / 102 / 102 |
| embed | 2 | 7381 / 7382 / 7391     | 8.4 / 8.5 / 8.5    | 11.7 / 11.8 / 11.9 | 241 / 241 / 242 |
| node  | 2 | 7127 / 7141 / 7196     | 8.3 / 8.3 / 8.3    | 17.1 / 17.2 / 17.3 | 228 / 229 / 229 |
| bun   | 2 | 5486 / 5496 / 5639     | 11.2 / 11.4 / 11.5 | 22.2 / 22.7 / 22.8 | 102 / 102 / 102 |
| embed | 4 | 10884 / 10901 / 11190  | 5.5 / 5.7 / 5.7    | 9.2 / 9.6 / 9.8    | 461 / 462 / 463 |
| node  | 4 | 10524 / 10704 / 10932  | 5.1 / 5.2 / 5.4    | 12.9 / 13.4 / 14.2 | 438 / 439 / 441 |
| bun   | 4 | 5477 / 5546 / 5595     | 11.2 / 11.3 / 11.5 | 22.3 / 22.5 / 22.9 | 101 / 101 / 101 |

256 connections:

| Host  | n | req/s                  | p50 (ms)       | p99 (ms)          | CPU (%)         |
| ----- | - | ---------------------- | -------------- | ----------------- | --------------- |
| embed | 1 | 3928 / 3953 / 3979     | 64.0 / 64.6 / 64.8 | 70.2 / 70.3 / 73.1 | 123 / 123 / 123 |
| node  | 1 | 3755 / 3778 / 3844     | 51.2 / 52.7 / 52.8 | 70.7 / 70.8 / 70.9 | 111 / 111 / 111 |
| bun   | 1 | 5394 / 5430 / 5485     | 46.7 / 47.3 / 47.6 | 49.2 / 49.5 / 49.8 | 102 / 102 / 102 |
| embed | 2 | 7387 / 7406 / 7410     | 33.9 / 34.0 / 34.1 | 40.8 / 41.9 / 43.0 | 241 / 241 / 242 |
| node  | 2 | 7083 / 7130 / 7229     | 34.0 / 34.8 / 34.9 | 46.1 / 46.1 / 49.6 | 220 / 220 / 222 |
| bun   | 2 | 5365 / 5398 / 5515     | 46.4 / 47.6 / 47.8 | 48.4 / 49.3 / 51.2 | 102 / 102 / 102 |
| embed | 4 | 10948 / 11010 / 11278  | 22.1 / 22.7 / 22.7 | 32.4 / 32.5 / 33.9 | 458 / 460 / 461 |
| node  | 4 | 10960 / 10985 / 11230  | 21.6 / 22.0 / 22.0 | 43.3 / 43.9 / 44.1 | 415 / 416 / 416 |
| bun   | 4 | 5366 / 5395 / 5591     | 46.6 / 47.2 / 47.8 | 49.9 / 50.1 / 67.6 | 101 / 101 / 102 |

- Embed against Node: at 64 connections, Embed serves 4.3, 3.4 and 1.8
  percent more requests per second at 1, 2 and 4 (ranges apart at 1 and 2;
  they overlap at 4). At 256 connections and 4, the two are equal within
  the spread. At 64 connections, the p99 of Embed is lower at each n (9.6
  against 13.4 ms at 4). At 256 connections and 1, the p50 of Embed is
  higher (64.6 against 52.7 ms) and the p99 is the same (70.3 against 70.8
  ms).
- Scaling from 1 to 2 and 4, at 64 connections: Embed 1.88 and 2.78 times,
  Node 1.90 and 2.85 times.
- CPU per request (CPU over req/s, 64 connections): Embed 313 us at 1 and
  424 us at 4; Node 301 us at 1 and 410 us at 4. Embed uses about 3 to 4
  percent more CPU per request: it parses HTTP on the tokio threads, on top
  of the worker threads. Both use more CPU per request at 4; the efficiency
  cores may take part of the work.
- Bun with n above 1 is not a scaling result. On macOS, SO_REUSEPORT does
  not spread the connections the way it does on Linux: one process gets all
  of them (one process at about 101 percent, the others at 0). No balancer
  was added in front of Bun.

### 24 - Boa

Embed with `--features boa` (openworkers-runtime-boa experiment/embed
ae85b91): a worker per request, as with nova. Page /, `run.sh`, 3 rounds
of 10 s, Embed on Boa (`embed`) and on V8 (`base`, a168e5d) in turn, one
worker thread. Load average 3.4 to 8.7. Raw lines: `results/24-boa.txt`.

| Host        | p50, 1 conn (ms)      | p99, 1 conn (ms)      | req/s, 1 conn | req/s, 64 conn     |
| ----------- | --------------------- | --------------------- | ------------- | ------------------ |
| embed (boa) | 44.68 / 44.74 / 44.76 | 46.51 / 46.91 / 47.63 | 22 / 22 / 22  | 29 / 29 / 29       |
| base (v8)   | 0.335 / 0.336 / 0.338 | 0.843 / 0.86 / 0.864  | 2790 to 2817  | 3949 / 3956 / 3956 |

- Boa is about 130 times slower than V8 on this page. Each request makes
  a new worker, which runs the whole bundle (145 KB) before the
  handler; the split between the two was not measured.
- The page is the same on both engines: 16400 bytes, status 200.

Size of `examples/rps` with `fetch,axum`, release, a168e5d (MB = 10^6
bytes):

| Engine | Size    | Stripped |
| ------ | ------- | -------- |
| v8     | 88.8 MB | 69.5 MB  |
| nova   | 25.8 MB | 20.8 MB  |
| boa    | 31.7 MB | 25.9 MB  |
| jsc    | 17.4 MB | 14.2 MB  |

jsc links the JavaScriptCore of macOS at run time, so its size does not
hold the engine.

### 25 - Nova

Embed with `--features nova,fetch,axum` (9f6a20c, openworkers-runtime-nova
v0.18.2, nova_vm v1.0.0-ow.2): a worker per request. Page /, `run.sh`, 3
rounds of 10 s, Embed on Nova (`embed`) and on V8 (`base`, 9f6a20c) in
turn, one worker thread. Load average 2.6 to 4.0. Raw lines:
`results/25-nova.txt`.

The build of the app does not run on Nova as is: page / answers 500.
`String.prototype.replace` substitutes nothing in a replacement template
that does not start with `$`: `"xy".replace(/(x)/, "[$1]")` gives `[$1]y`.
In GetSubstitution of nova_vm (`string_prototype.rs`), a template rest
that does not start with `$` is taken whole as `ref`, where step 5.h takes
one character. The app rewrites its SQL with
`.replace(/(\w+)::float/g, "CAST($1 AS REAL)")`; the `$1` left in the query
becomes a `?`, and SQLite refuses the leaderboard query ("Got 0, needed 1").

The bench runs a copy of the build where that one call takes a function,
`(m, g) => "CAST(" + g + " AS REAL)"`, on both engines.

| Host         | p50, 1 conn (ms)      | p99, 1 conn (ms)       | req/s, 1 conn | req/s, 64 conn     |
| ------------ | --------------------- | ---------------------- | ------------- | ------------------ |
| embed (nova) | 45.35 / 46.42 / 46.15 | 56.49 / 71.15 / 52.84  | 22 / 21 / 21  | 28 / 28 / 28       |
| base (v8)    | 0.339 / 0.338 / 0.339 | 0.944 / 0.914 / 0.911  | 2716 to 2750  | 3751 / 3811 / 3768 |

- Nova is about 135 times slower than V8 on this page, as Boa (section
  24). Each request makes a new worker, which runs the whole bundle (145
  KB) before the handler; the split between the two was not measured.
- The page is the same on both engines, and the same as the page of the
  build as is on V8: 16400 bytes, status 200.
- CPU: not measured. nova_vm 1.0 has no interrupt API and no instruction
  budget (runtime-nova README), so a loop that never ends is not cut.
