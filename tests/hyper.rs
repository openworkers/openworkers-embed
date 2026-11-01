#![cfg(feature = "hyper")]

mod common;

use bytes::Bytes;
use common::{CLIENT, ECHO};
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

    let response = handle(&embed, request, "192.0.2.7:4000".parse().unwrap()).await;

    assert_eq!(response.status(), 201);
    assert_eq!(response.headers()["x-echo"], "yes");

    let body = response.into_body().collect().await.unwrap().to_bytes();

    assert_eq!(body, "POST /play cookie=a=1; b=2 body=rock");
}

// The client says it is 1.2.3.4 over https: the worker sees the peer and the
// scheme of the listener. A peer in IPv4-mapped IPv6 reads as IPv4
#[tokio::test]
async fn the_worker_sees_the_peer_and_not_the_headers_of_the_client() {
    let embed = Embed::builder(CLIENT).threads(1).build().unwrap();

    for peer in ["192.0.2.7:4000", "[::ffff:192.0.2.7]:4000"] {
        let request = hyper::Request::get("/")
            .header("x-real-ip", "1.2.3.4")
            .header("X-Forwarded-Proto", "https")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let response = handle(&embed, request, peer.parse().unwrap()).await;
        let body = response.into_body().collect().await.unwrap().to_bytes();

        assert_eq!(body, "192.0.2.7 http http:", "{peer}");
    }
}
