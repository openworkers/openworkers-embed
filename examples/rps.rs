//! Embed host of the rock-paper-scissors bench: Axum in front, the worker of
//! the build behind, and SQLite for `platform.invoke('query')`.
//!
//!     cargo run --release --features axum --example rps <build-dir> <database>
//!
//! `THREADS` sets the worker threads (default 1), `PORT` the port (default 3000).
//! `benches/rps/README.md` tells how to make the build and the database.

use openworkers_embed::Embed;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::OnceLock;

static DATABASE: OnceLock<PathBuf> = OnceLock::new();

thread_local! {
    // One connection per thread, opened on first use, as Node and Bun keep one
    static CONNECTION: RefCell<Option<Connection>> = const { RefCell::new(None) };
}

#[derive(Deserialize)]
struct Query {
    sql: String,
    params: Vec<Value>,
}

fn param(value: &Value) -> rusqlite::types::Value {
    match value {
        Value::Null => rusqlite::types::Value::Null,
        Value::Bool(b) => rusqlite::types::Value::Integer(*b as i64),
        Value::Number(n) => match n.as_i64() {
            Some(i) => rusqlite::types::Value::Integer(i),
            None => rusqlite::types::Value::Real(n.as_f64().unwrap_or(0.0)),
        },
        Value::String(s) => rusqlite::types::Value::Text(s.clone()),
        other => rusqlite::types::Value::Text(other.to_string()),
    }
}

fn column(value: ValueRef) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => Value::from(i),
        ValueRef::Real(f) => Value::from(f),
        ValueRef::Text(t) => Value::from(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => Value::from(b.to_vec()),
    }
}

fn query(connection: &Connection, query: Query) -> rusqlite::Result<Vec<Value>> {
    let mut statement = connection.prepare_cached(&query.sql)?;
    let names: Vec<String> = statement
        .column_names()
        .into_iter()
        .map(String::from)
        .collect();
    let params: Vec<rusqlite::types::Value> = query.params.iter().map(param).collect();
    let mut rows = statement.query(rusqlite::params_from_iter(params))?;
    let mut out = Vec::new();

    while let Some(row) = rows.next()? {
        let mut object = Map::new();

        for (index, name) in names.iter().enumerate() {
            object.insert(name.clone(), column(row.get_ref(index)?));
        }

        out.push(Value::Object(object));
    }

    Ok(out)
}

fn run(args: Query) -> Result<Vec<Value>, String> {
    CONNECTION.with(|cell| {
        let mut cell = cell.borrow_mut();

        if cell.is_none() {
            let path = DATABASE.get().expect("the database path is set");
            let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(|e| e.to_string())?;

            *cell = Some(connection);
        }

        query(cell.as_ref().expect("opened above"), args).map_err(|e| e.to_string())
    })
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1).map(PathBuf::from);
    let build = args.next().expect("usage: rps <build-dir> <database>");
    let database = args.next().expect("usage: rps <build-dir> <database>");
    let threads = std::env::var("THREADS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(1);

    DATABASE.set(database).expect("set once");

    let code = std::fs::read_to_string(build.join("_worker.js")).expect("build/_worker.js");
    let embed = Embed::builder(code)
        .threads(threads)
        .assets(build.join("assets"))
        .command_sync("query", run)
        .build()
        .expect("worker code");

    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .unwrap();

    println!("listening on http://127.0.0.1:{port} with {threads} worker thread(s)");
    axum::serve(listener, openworkers_embed::backend::axum::router(embed))
        .await
        .unwrap();
}
