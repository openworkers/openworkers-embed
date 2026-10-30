#![cfg(feature = "hyper")]

mod common;

use bytes::Bytes;
use common::ECHO;
use http_body_util::{BodyExt, Full};
use openworkers_embed::Embed;
use openworkers_embed::backend::hyper::handle;

#[tokio::test]
async fn a_hyper_request_reaches_the_worker_and_comes_back() {
    let embed = Embed::builder(ECHO).threads(1).build().unwrap();

    let request = hyper::Request::post("/play?x=1")
        .header("cookie", "a=1")
        .header("cookie", "b=2")
        .body(Full::new(Bytes::from("rock")))
        .unwrap();

    let response = handle(&embed, request).await;

    assert_eq!(response.status(), 201);
    assert_eq!(response.headers()["x-echo"], "yes");

    let body = response.into_body().collect().await.unwrap().to_bytes();

    assert_eq!(body, "POST /play cookie=a=1; b=2 body=rock");
}
