//! Requests through `Embed::fetch`, with no HTTP in front: the engine, the
//! worker pool and the commands. Run it once per engine:
//!
//!     cargo bench --no-default-features --features nova
//!
//! The SSR cases need the build of `examples/sveltekit-app` and are skipped
//! without it.
//!
//! The time budgets keep each case under about 300 000 requests per worker thread:
//! past that, the contexts the v8 pool retired fill its heap.

use criterion::{Criterion, criterion_group, criterion_main};
use openworkers_embed::{Embed, HttpMethod, HttpRequest, RequestBody};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinSet;

const CONCURRENT: usize = 64;

const HELLO: &str = "export default { fetch: () => new Response('hello') };";

const INVOKE: &str = "export default {
    async fetch() {
        const sum = await embed.invoke('add', { a: 1, b: 2 });

        return new Response(String(sum));
    },
};";

#[derive(Deserialize)]
struct Pair {
    a: u32,
    b: u32,
}

fn request(path: &str) -> HttpRequest {
    HttpRequest {
        method: HttpMethod::Get,
        url: format!("http://localhost{path}"),
        headers: HashMap::new(),
        body: RequestBody::None,
    }
}

fn build_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/sveltekit-app/build")
}

fn embed(code: &str, assets: Option<PathBuf>, threads: usize) -> Arc<Embed> {
    let mut builder = Embed::builder(code)
        .threads(threads)
        .command_sync("add", |pair: Pair| Ok::<_, String>(pair.a + pair.b))
        .command_sync("greet", |_: serde_json::Value| {
            Ok::<_, String>(serde_json::json!({ "message": "hello from Rust" }))
        })
        .command_sync("hit", |_: ()| Ok::<_, String>(1));

    if let Some(assets) = assets {
        builder = builder.assets(assets);
    }

    Arc::new(builder.build().expect("worker code"))
}

fn ssr(threads: usize) -> Option<Arc<Embed>> {
    let build = build_dir();
    let code = std::fs::read_to_string(build.join("_worker.js")).ok()?;

    Some(embed(&code, Some(build.join("assets")), threads))
}

fn bench_latency(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("latency");

    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));

    for (name, code) in [("hello", HELLO), ("invoke", INVOKE)] {
        let embed = embed(code, None, 1);

        group.bench_function(name, |b| {
            b.to_async(&runtime)
                .iter(|| async { embed.fetch(request("/")).await.unwrap() })
        });
    }

    match ssr(1) {
        Some(embed) => {
            group.bench_function("ssr", |b| {
                b.to_async(&runtime)
                    .iter(|| async { embed.fetch(request("/?name=Max")).await.unwrap() })
            });
        }
        None => eprintln!("skipping ssr: no build in examples/sveltekit-app/build"),
    }

    group.finish();
}

fn bench_throughput(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let mut group = c.benchmark_group(format!("{CONCURRENT}_concurrent"));

    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(8));

    let hello = embed(HELLO, None, threads);

    group.bench_function("hello", |b| {
        b.to_async(&runtime).iter(|| burst(hello.clone(), "/"))
    });

    if let Some(embed) = ssr(threads) {
        group.bench_function("ssr", |b| {
            b.to_async(&runtime)
                .iter(|| burst(embed.clone(), "/?name=Max"))
        });
    }

    group.finish();
}

/// `CONCURRENT` requests in flight at once, all awaited.
async fn burst(embed: Arc<Embed>, path: &'static str) {
    let mut set = JoinSet::new();

    for _ in 0..CONCURRENT {
        let embed = embed.clone();

        set.spawn(async move { embed.fetch(request(path)).await.unwrap() });
    }

    while let Some(response) = set.join_next().await {
        response.unwrap();
    }
}

criterion_group!(benches, bench_latency, bench_throughput);
criterion_main!(benches);
