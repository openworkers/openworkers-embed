# openworkers-embed

Run a worker inside a Rust program. The worker is an ES module with `export default { fetch }`, for example the output of `@openworkers/adapter-sveltekit`. It runs in the same process as your Rust code, on V8 by default.

The worker calls Rust functions that you register by name, and your Rust code owns storage, databases and any other resource. There are no Cloudflare-style bindings to configure.

## Use

```rust
use openworkers_embed::Embed;

let embed = Embed::builder(std::fs::read_to_string("build/_worker.js")?)
    .assets("build/assets")
    .command_sync("greet", |name: String| Ok::<_, String>(format!("hello {name}")))
    .build()?;
```

The worker calls the command through its `EMBED` binding:

```js
export default {
  async fetch(request, env) {
    return new Response(await env.EMBED.invoke("greet", "Ada"));
  },
};
```

In SvelteKit, `@openworkers/adapter-sveltekit` 0.6.2 or later adds `platform.invoke`, which calls `env.EMBED.invoke`:

```js
export const load = async ({ platform }) => ({
  user: await platform.invoke("get_user", { id: 42 }),
});
```

- Arguments and results are JSON. A command that returns `Err` throws an `Error` in the worker.
- `command` takes an async function. `command_sync` takes a function that does not wait.
- A function that answers at once runs inside the turn of the worker's JavaScript: its time counts in the CPU limit of the request, 50 ms by default.
- `assets(dir)` serves the files of `dir` for GET and HEAD requests, before the worker runs. The worker also reads them as `env.ASSETS`. The names of the files are indexed at start, and a file is read on each request. A prerendered page added after start waits for a restart.
- The `fetch` of the worker goes out over HTTP, with the client settings of the runner: a redirect is the answer and not a hop, a pause of 10 seconds from the server ends the call, and the body streams back. There is no egress filter, because the code is trusted. The `fetch` feature, on by default, brings `reqwest`.
- A response of the worker streams out: `Embed::fetch` returns as soon as the worker has one, and the body follows while the worker keeps running. A client that drops the body ends the stream, and a worker that the runtime stops while its body streams ends the body with an error. nova and boa drain a stream before they answer, so an endless stream never ends there.

## Engines

The JavaScript engine is a feature. Exactly one is active; V8 is the default.

| Feature | Engine         | Notes                                                                                                                                                                               |
| ------- | -------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `v8`    | V8             | Isolates stay warm in a pool. Serves every binding `embed` uses.                                                                                                                    |
| `nova`  | Nova           | A worker per request. Serves `Assets` and `Invoke`. It has no global `fetch`. `String.prototype.replace` substitutes nothing in a replacement template that does not start with `$`: `"xy".replace(/(x)/, "[$1]")` gives `[$1]y`. |
| `boa`   | Boa            | A worker per request, which runs the whole bundle again for each request: page `/` of the bench takes 45 ms, against 0.34 ms on V8. Serves `Assets` and `Invoke`, and a global `fetch` that reads a reply to its end before the worker gets it. Boa has no CPU limit and cannot interrupt a running script: a loop that never ends keeps its worker thread for good. The wall clock, 30 s, ends a request only between two jobs. |
| `jsc`   | JavaScriptCore | Warm workers per thread, on openworkers-jsc (the system framework of macOS, or with the `openworkers-runtime-jsc/static` feature the static JavaScriptCore of oven-sh), reset between requests, under a CPU budget per request. The watchdog of JavaScriptCore does not cut native code such as a regular expression that backtracks. Serves `Invoke` only, so `env.ASSETS` does not exist: the files of `assets(dir)` are served by Rust before the worker runs. |

On macOS, the system JavaScriptCore runs JavaScript without its JIT unless the binary has the `com.apple.security.cs.allow-jit` entitlement: page `/` of the bench is about 5 times slower without it. To sign a binary with it:

```bash
cat > jit.entitlements <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>com.apple.security.cs.allow-jit</key><true/></dict></plist>
PLIST
codesign -s - --entitlements jit.entitlements -o runtime --force target/release/your-binary
```

With the JIT, the CPU limit cuts JIT code only with polling traps. The jsc engine turns them on when the Embed is built, before its worker threads start; the build fails if JavaScriptCore already started without them. The option goes through the environment, so build the Embed early, before other threads start, or call `openworkers_runtime_jsc::init_jsc` with `polling_traps` yourself before then. Measured on page `/` of the bench (Apple M2, one thread): 382 us at 1 connection with the JIT, 2.1 ms without, against 333 us with V8.

```bash
cargo test --no-default-features --features nova,axum
cargo test --no-default-features --features boa,fetch,axum
```

## Web frameworks

`Embed::fetch` takes an `HttpRequest` and returns an `HttpResponse` from `openworkers-core`. For a web framework, enable its feature:

| Feature | Entry point                                                        |
| ------- | ------------------------------------------------------------------ |
| `actix` | `backend::actix::handle`, used with `web::to` as a default service |
| `hyper` | `backend::hyper::handle(&embed, request)`                          |
| `axum`  | `backend::axum::router(embed)`                                     |

## Example

```bash
cargo run --features axum --example server [build-dir]
```

The build directory holds `_worker.js` and `assets/`. Without it, the example serves a small worker.

## Build

With `v8`, `openworkers-runtime-v8` reads `RUNTIME_SNAPSHOT_PATH` at compile time. Set it in `.cargo/config.toml` of your program, as this repository does. When it starts, the runtime reads the V8 snapshot from that path: the bindings of the runtime, compiled once. An empty or missing file means no snapshot: the runtime starts slower and still works.

Write the snapshot before you run, and again after each update of `openworkers-runtime-v8`:

```bash
cargo run --release --bin snapshot
```

A program that uses this crate calls `openworkers_embed::snapshot::write()` from a binary of its own, built with the same `openworkers-runtime-v8`.

## Limits

- A file of `assets(dir)` is read to the end before it is sent.
- The worker code is trusted. On v8 there is no wall-clock limit, so a stream may live as long as its client reads it. On jsc the wall clock is the default of the runtime, 30 s, so a stream stops after 30 s. On boa nothing cuts a script that runs without end, at the top level or in a request: its thread stays busy for good. The CPU and memory limits are the defaults of the runtime.
- One worker per `Embed`. Replacing the code means building a new `Embed`.
- On v8, one thread runs 8 requests at the same time, one per place: an isolate with its one context. A ninth request waits for a place. A stream keeps its place until the client has read it, so 8 open streams fill a thread.
- A request goes to the thread with the fewest requests in flight.
- A context is never retired, on v8 and on jsc. It keeps its globals for the life of the process.
