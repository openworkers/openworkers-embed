//! A complete server: Axum in front, a worker behind, and two Rust commands the
//! worker calls with `embed.invoke`.
//!
//! Run it with `cargo run --features axum --example server [build-dir]`, then
//! `curl localhost:3000/?name=Max`. Set `PORT` to listen elsewhere. The build
//! directory is the output of `@openworkers/adapter-sveltekit`: `_worker.js` and
//! `assets/`.

use openworkers_embed::Embed;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

const WORKER: &str = r#"
export default {
    async fetch(request) {
        const name = new URL(request.url).searchParams.get('name') ?? 'world';
        const user = await embed.invoke('greet', { name });
        const hits = await embed.invoke('hit');

        return new Response(`${user.message}, hit ${hits}\n`);
    },
};
"#;

#[derive(Deserialize)]
struct Greet {
    name: String,
}

#[derive(Serialize)]
struct Greeting {
    message: String,
}

#[tokio::main]
async fn main() {
    let hits = Arc::new(AtomicU64::new(0));
    let build = std::env::args().nth(1).map(PathBuf::from);

    let code = match &build {
        Some(dir) => std::fs::read_to_string(dir.join("_worker.js")).expect("build/_worker.js"),
        None => WORKER.to_string(),
    };

    let mut builder = Embed::builder(code);

    if let Some(dir) = &build {
        builder = builder.assets(dir.join("assets"));
    }

    let embed = builder
        .command("greet", |args: Greet| async move {
            Ok::<_, String>(Greeting {
                message: format!("hello {} from Rust", args.name),
            })
        })
        .command("hit", move |_: ()| {
            let hits = hits.clone();

            async move { Ok::<_, String>(hits.fetch_add(1, Ordering::Relaxed) + 1) }
        })
        .build()
        .expect("worker code");

    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .unwrap();

    println!("listening on http://127.0.0.1:{port}");
    axum::serve(listener, openworkers_embed::backend::axum::router(embed))
        .await
        .unwrap();
}
