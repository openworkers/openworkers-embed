//! A worker runs on the snapshot that `snapshot::write` makes. This test
//! writes the snapshot of the build to `RUNTIME_SNAPSHOT_PATH`, before any
//! isolate of this process reads it.

#![cfg(feature = "v8")]

mod common;

use common::{ECHO, get, text};
use openworkers_embed::{Embed, snapshot};

#[tokio::test]
async fn a_worker_runs_on_the_written_snapshot() {
    let path = snapshot::write().expect("the snapshot is made");

    assert!(std::fs::metadata(path).unwrap().len() > 0);

    let embed = Embed::builder(ECHO).threads(1).build().unwrap();
    let response = embed.fetch(get("/a")).await.unwrap();

    assert_eq!(response.status, 201);
    assert_eq!(text(response).await, "GET /a cookie=null body=");
}
