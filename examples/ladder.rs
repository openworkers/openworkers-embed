//! The layer ladder of the rock-paper-scissors bench: each mode adds one layer,
//! so the difference between two modes is the cost of that layer.
//!
//!     cargo run --release --features axum --example ladder <mode> [worker.js]
//!
//! Modes:
//!   axum     Axum answers "hello"
//!   hop      Axum sends a job to one JS-less thread, as the pool does, and waits
//!   embed    Embed with the worker of the file, one thread
//!
//! With `ASSETS=<dir>`, Embed looks for a file in that directory first, as
//! examples/rps.rs does. The command `query` answers `[{"x": 1}]`.

use axum::Router;
use axum::body::Body;
use axum::response::Response;
use openworkers_embed::Embed;
use std::future::Future;
use std::pin::Pin;
use tokio::sync::{mpsc, oneshot};

type Job = Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()>>> + Send>;

/// One thread with a current-thread runtime and a LocalSet, as in src/pool.rs.
fn spawn() -> mpsc::UnboundedSender<Job> {
    let (sender, mut jobs) = mpsc::unbounded_channel::<Job>();

    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();

        local.block_on(&runtime, async move {
            while let Some(job) = jobs.recv().await {
                tokio::task::spawn_local(job());
            }
        });
    });

    sender
}

fn hop() -> Router {
    let sender = spawn();

    Router::new().fallback(move || {
        let sender = sender.clone();

        async move {
            let (done, result) = oneshot::channel();
            let job: Job = Box::new(move || {
                Box::pin(async move {
                    let _ = done.send("hello");
                })
            });

            sender.send(job).unwrap();

            Response::new(Body::from(result.await.unwrap()))
        }
    })
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let mode = args.next().expect("usage: ladder <mode> [worker.js]");

    let router = match mode.as_str() {
        "axum" => Router::new().fallback(|| async { "hello" }),
        "hop" => hop(),
        "embed" => {
            let file = args.next().expect("usage: ladder embed <worker.js>");
            let code = std::fs::read_to_string(file).expect("worker file");
            let mut builder = Embed::builder(code)
                .threads(1)
                .command_sync("query", |_: serde_json::Value| {
                    Ok::<_, String>(serde_json::json!([{ "x": 1 }]))
                });

            if let Ok(dir) = std::env::var("ASSETS") {
                builder = builder.assets(dir);
            }

            openworkers_embed::backend::axum::router(builder.build().expect("worker code"))
        }
        other => panic!("unknown mode {other}"),
    };

    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .unwrap();

    println!("listening on http://127.0.0.1:{port}");
    axum::serve(listener, router).await.unwrap();
}
