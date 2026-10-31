//! The server of the benchmarks in `bench/`: Axum in front, one worker thread,
//! and the three SQLite reads of the bench app as commands.
//!
//! `cargo run --release --features axum --example bench_server`, after
//! `node bench/seed.mjs` and the build of `bench/app`. Environment:
//!
//! - `MODE`: `app` (default) the bench app, `hello` a worker with a fixed body,
//!   `invoke` a worker that calls one command, `axum` a fixed body with no worker.
//! - `THREADS`: worker threads, default 1.
//! - `PORT`: default 3000.

use openworkers_embed::Embed;
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const HELLO: &str = "export default { fetch: () => new Response('hello') };";

const INVOKE: &str = "export default {
    async fetch(request, env) {
        const totals = await env.EMBED.invoke('totals');

        return new Response(String(totals.games));
    },
};";

// The SQL of bench/queries.mjs
const LEADERS: &str =
    "SELECT name, wins, losses, draws FROM players ORDER BY wins DESC, id LIMIT 10";
const RECENT: &str = "SELECT g.id, p.name, g.player, g.server, g.result
  FROM games g JOIN players p ON p.id = g.player_id ORDER BY g.id DESC LIMIT 10";
const TOTALS: &str = "SELECT games, wins, losses, draws FROM totals WHERE id = 1";

#[derive(Serialize)]
struct Leader {
    name: String,
    wins: i64,
    losses: i64,
    draws: i64,
}

#[derive(Serialize)]
struct Game {
    id: i64,
    name: String,
    player: String,
    server: String,
    result: String,
}

#[derive(Serialize)]
struct Totals {
    games: i64,
    wins: i64,
    losses: i64,
    draws: i64,
}

fn bench_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("bench")
}

fn leaders(db: &Connection) -> rusqlite::Result<Vec<Leader>> {
    let mut statement = db.prepare_cached(LEADERS)?;
    let rows = statement.query_map([], |row| {
        Ok(Leader {
            name: row.get(0)?,
            wins: row.get(1)?,
            losses: row.get(2)?,
            draws: row.get(3)?,
        })
    })?;

    rows.collect()
}

fn recent(db: &Connection) -> rusqlite::Result<Vec<Game>> {
    let mut statement = db.prepare_cached(RECENT)?;
    let rows = statement.query_map([], |row| {
        Ok(Game {
            id: row.get(0)?,
            name: row.get(1)?,
            player: row.get(2)?,
            server: row.get(3)?,
            result: row.get(4)?,
        })
    })?;

    rows.collect()
}

fn totals(db: &Connection) -> rusqlite::Result<Totals> {
    db.prepare_cached(TOTALS)?.query_row([], |row| {
        Ok(Totals {
            games: row.get(0)?,
            wins: row.get(1)?,
            losses: row.get(2)?,
            draws: row.get(3)?,
        })
    })
}

#[tokio::main]
async fn main() {
    let mode = std::env::var("MODE").unwrap_or_else(|_| "app".to_string());
    let threads = std::env::var("THREADS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(1);
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .unwrap();

    println!("{mode} on http://127.0.0.1:{port}, {threads} thread(s)");

    if mode == "axum" {
        let router = axum::Router::new().fallback(|| async { "hello" });

        axum::serve(listener, router).await.unwrap();

        return;
    }

    let build = bench_dir().join("app/build");
    let code = match mode.as_str() {
        "hello" => HELLO.to_string(),
        "invoke" => INVOKE.to_string(),
        _ => std::fs::read_to_string(build.join("_worker.js")).expect("bench/app/build/_worker.js"),
    };

    let db =
        Connection::open_with_flags(bench_dir().join("rps.db"), OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("bench/rps.db: run node bench/seed.mjs");
    let db = Arc::new(Mutex::new(db));
    let (a, b, c) = (db.clone(), db.clone(), db);

    let mut builder = Embed::builder(code)
        .threads(threads)
        .command_sync("leaders", move |_: ()| {
            leaders(&a.lock().unwrap()).map_err(|e| e.to_string())
        })
        .command_sync("recent", move |_: ()| {
            recent(&b.lock().unwrap()).map_err(|e| e.to_string())
        })
        .command_sync("totals", move |_: ()| {
            totals(&c.lock().unwrap()).map_err(|e| e.to_string())
        });

    if mode == "app" {
        builder = builder.assets(build.join("assets"));
    }

    let embed = builder.build().expect("worker code");

    axum::serve(listener, openworkers_embed::backend::axum::router(embed))
        .await
        .unwrap();
}
