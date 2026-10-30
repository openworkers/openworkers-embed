# openworkers-embed

Run a worker inside a Rust program. The worker is an ES module with `export default { fetch }`, for example the output of `@openworkers/adapter-sveltekit`. It runs in V8, in the same process as your Rust code.

The worker calls Rust functions that you register by name, and your Rust code owns storage, databases and any other resource. There are no Cloudflare-style bindings to configure.

## Use

```rust
use openworkers_embed::Embed;

let embed = Embed::builder(std::fs::read_to_string("build/_worker.js")?)
    .assets("build/assets")
    .command_sync("greet", |name: String| Ok::<_, String>(format!("hello {name}")))
    .build()?;
```

The worker calls the command:

```js
const message = await embed.invoke('greet', 'Ada');
```

- Arguments and results are JSON. A command that returns `Err` throws an `Error` in the worker.
- `command` takes an async function. `command_sync` takes a function that does not wait.
- `assets(dir)` serves the files of `dir` for GET and HEAD requests, before the worker runs. The worker also reads them as `env.ASSETS`. Files are read on each request.

## Engines

The JavaScript engine is a feature. Exactly one is active; V8 is the default.

| Feature | Engine | Notes |
| ------- | ------ | ----- |
| `v8` | V8 | Isolates stay warm in a pool. Serves every binding `embed` uses. |
| `nova` | Nova | A worker per request. Serves `Assets` and `Invoke`. |
| `jsc` | JavaScriptCore | A worker per request, on the system framework of macOS. Serves `Invoke` only, so `env.ASSETS` does not exist: the files of `assets(dir)` are served by Rust before the worker runs. |

```bash
cargo test --no-default-features --features nova,axum
```

## Web frameworks

`Embed::fetch` takes an `HttpRequest` and returns an `HttpResponse` from `openworkers-core`. For a web framework, enable its feature:

| Feature | Entry point                                                       |
| ------- | ----------------------------------------------------------------- |
| `actix` | `backend::actix::handle`, used with `web::to` as a default service |
| `hyper` | `backend::hyper::handle(&embed, request)`                         |
| `axum`  | `backend::axum::router(embed)`                                    |

## Example

```bash
cargo run --features axum --example server [build-dir]
```

The build directory holds `_worker.js` and `assets/`. Without it, the example serves a small worker.

## Build

With `v8`, `openworkers-runtime-v8` reads `RUNTIME_SNAPSHOT_PATH` at compile time. Set it in `.cargo/config.toml` of your program, as this repository does. An empty file means no snapshot: the runtime starts slower and still works.

## Limits

- Responses are read to the end before they leave: no streaming.
- The worker code is trusted. There are no per-request CPU or memory limits beyond the runtime defaults.
- One worker per `Embed`. Replacing the code means building a new `Embed`.
