#![cfg(feature = "hyper")]

mod common;

use bytes::Bytes;
use common::{CLIENT, ECHO, RUNNER_HEADERS, client_seen};
use http_body_util::{BodyExt, Full};
use openworkers_embed::Embed;
use openworkers_embed::backend::hyper::{handle, handle_with};
use openworkers_embed::backend::{Connection, Scheme};

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

// The client sends every header of the runner, as if it were 1.2.3.4 over
// https: the worker sees only the peer and the scheme of the listener. A
// peer in IPv4-mapped IPv6 reads as IPv4
#[tokio::test]
async fn the_worker_sees_the_peer_and_not_the_headers_of_the_client() {
    let embed = Embed::builder(CLIENT).threads(1).build().unwrap();

    for peer in ["192.0.2.7:4000", "[::ffff:192.0.2.7]:4000"] {
        let mut request = hyper::Request::get("/x").header("host", "app.test");

        for name in RUNNER_HEADERS {
            request = request.header(name, "1.2.3.4");
        }

        let request = request
            .header("X-Forwarded-Proto", "https")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let connection = Connection {
            peer: Some(peer.parse().unwrap()),
            scheme: Scheme::Http,
        };
        let response = handle_with(&embed, request, connection).await;
        let body = response.into_body().collect().await.unwrap().to_bytes();

        assert_eq!(
            body,
            client_seen("http://app.test/x", Some("192.0.2.7"), "http"),
            "{peer}"
        );
    }
}

// handle knows no peer: the worker gets no address, not the client's
#[tokio::test]
async fn handle_gives_no_address() {
    let embed = Embed::builder(CLIENT).threads(1).build().unwrap();
    let request = hyper::Request::get("/x")
        .header("host", "app.test")
        .header("x-forwarded-for", "6.6.6.6")
        .body(Full::new(Bytes::new()))
        .unwrap();

    let response = handle(&embed, request).await;
    let body = response.into_body().collect().await.unwrap().to_bytes();

    assert_eq!(body, client_seen("http://app.test/x", None, "http"));
}

// Behind a proxy that ends TLS, the host says https: the URL and
// x-forwarded-proto carry it
#[tokio::test]
async fn the_host_sets_the_scheme() {
    let embed = Embed::builder(CLIENT).threads(1).build().unwrap();
    let request = hyper::Request::get("/x")
        .header("host", "app.test")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let connection = Connection {
        peer: Some("192.0.2.7:4000".parse().unwrap()),
        scheme: Scheme::Https,
    };

    let response = handle_with(&embed, request, connection).await;
    let body = response.into_body().collect().await.unwrap().to_bytes();

    assert_eq!(
        body,
        client_seen("https://app.test/x", Some("192.0.2.7"), "https")
    );
}

// HTTP/2 carries the host in :authority and sends no Host header: the URL
// takes the authority, not localhost
#[tokio::test]
async fn with_no_host_header_the_url_takes_the_authority() {
    let embed = Embed::builder(CLIENT).threads(1).build().unwrap();
    let request = hyper::Request::get("https://app.test/x?q=1")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let connection = Connection {
        peer: None,
        scheme: Scheme::Https,
    };

    let response = handle_with(&embed, request, connection).await;
    let body = response.into_body().collect().await.unwrap().to_bytes();

    assert_eq!(body, client_seen("https://app.test/x?q=1", None, "https"));
}

// The Host header goes into the URL: one with a path, a query, a userinfo,
// nothing or a bad port is refused before the worker
#[tokio::test]
async fn a_bad_host_is_refused() {
    let embed = Embed::builder(CLIENT).threads(1).build().unwrap();

    for host in [
        "evil.example/admin?",
        "app.test?x",
        "user@evil.example",
        "",
        "app.test:99999",
    ] {
        let request = hyper::Request::get("/x")
            .header("host", host)
            .body(Full::new(Bytes::new()))
            .unwrap();

        assert_eq!(handle(&embed, request).await.status(), 400, "{host:?}");
    }
}
