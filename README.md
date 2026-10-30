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
- `assets(dir)` serves the files of `dir` for GET and HEAD requests, before the worker runs. The worker also reads them as `env.ASSETS`. Files are read on each request.
- The `fetch` of the worker goes out over HTTP, with the client settings of the runner: a redirect is the answer and not a hop, a pause of 10 seconds from the server ends the call, and the body streams back. There is no egress filter, because the code is trusted. The `fetch` feature, on by default, brings `reqwest`.
- A response of the worker streams out: `Embed::fetch` returns as soon as the worker has one, and the body follows while the worker keeps running. A client that drops the body ends the stream, and a worker that the runtime stops while its body streams ends the body with an error. nova drains a stream before it answers, so an endless stream never ends there.

## Engines

The JavaScript engine is a feature. Exactly one is active; V8 is the default.

| Feature | Engine         | Notes                                                                                                                                                                               |
| ------- | -------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `v8`    | V8             | Isolates stay warm in a pool. Serves every binding `embed` uses.                                                                                                                    |
| `nova`  | Nova           | A worker per request. Serves `Assets` and `Invoke`. It has no global `fetch`.                                                                                                       |
| `jsc`   | JavaScriptCore | A worker per request, on the system framework of macOS. Serves `Invoke` only, so `env.ASSETS` does not exist: the files of `assets(dir)` are served by Rust before the worker runs. |

```bash
cargo test --no-default-features --features nova,axum
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

With `v8`, `openworkers-runtime-v8` reads `RUNTIME_SNAPSHOT_PATH` at compile time. Set it in `.cargo/config.toml` of your program, as this repository does. An empty file means no snapshot: the runtime starts slower and still works.

## Limits

- A file of `assets(dir)` is read to the end before it is sent.
- The worker code is trusted. On v8 there is no wall-clock limit, so a stream may live as long as its client reads it. The CPU and memory limits are the defaults of the runtime.
- One worker per `Embed`. Replacing the code means building a new `Embed`.
