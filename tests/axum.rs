#![cfg(feature = "axum")]

mod common;

use axum::body::Body;
use axum::http::Request;
use common::{CLIENT, ECHO};
use http_body_util::BodyExt;
use openworkers_embed::Embed;
use openworkers_embed::backend::axum::router;
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower::ServiceExt;

#[tokio::test]
async fn an_axum_router_sends_requests_to_the_worker() {
    let embed = Embed::builder(ECHO).threads(1).build().unwrap();

    let request = Request::post("/play")
        .header("cookie", "a=1")
        .body(Body::from("paper"))
        .unwrap();

    let response = router(embed).oneshot(request).await.unwrap();

    assert_eq!(response.status(), 201);

    let body = response.into_body().collect().await.unwrap().to_bytes();

    assert_eq!(body, "POST /play cookie=a=1 body=paper");
}

// The Host header goes into the URL of the request; the file served comes from
// the path of the request line, not from the Host
#[tokio::test]
async fn the_host_header_does_not_choose_the_file() {
    let site = tempfile::tempdir().unwrap();

    std::fs::create_dir_all(site.path().join(".well-known")).unwrap();
    std::fs::write(site.path().join("index.html"), "home").unwrap();
    std::fs::write(site.path().join(".well-known/security.txt"), "contact").unwrap();

    let embed = Embed::builder(ECHO)
        .threads(1)
        .assets(site.path())
        .build()
        .unwrap();
    let request = Request::get("/index.html")
        .header("host", "x/.well-known/security.txt?")
        .body(Body::empty())
        .unwrap();
    let response = router(embed).oneshot(request).await.unwrap();
    let body = response.into_body().collect().await.unwrap().to_bytes();

    assert_eq!(body, "home");
}

// A client on a real socket says it is 1.2.3.4 over https: the worker sees
// its address, 127.0.0.1, and http
#[tokio::test]
async fn the_worker_sees_the_peer_and_not_the_headers_of_the_client() {
    let embed = Embed::builder(CLIENT).threads(1).build().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = router(embed).into_make_service_with_connect_info::<SocketAddr>();

    tokio::spawn(async move { axum::serve(listener, app).await });

    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();

    stream
        .write_all(
            b"GET / HTTP/1.1\r\nhost: x\r\nx-real-ip: 1.2.3.4\r\n\
              x-forwarded-proto: https\r\nconnection: close\r\n\r\n",
        )
        .await
        .unwrap();

    let mut answer = String::new();

    stream.read_to_string(&mut answer).await.unwrap();

    // The body comes chunked: its one chunk is the line of the worker
    assert!(answer.contains("\r\n127.0.0.1 http http:\r\n"), "{answer}");
}

// Served without the connect info, the router knows no peer: the worker gets
// no x-real-ip, not the one of the client
#[tokio::test]
async fn without_the_connect_info_the_worker_gets_no_address() {
    let embed = Embed::builder(CLIENT).threads(1).build().unwrap();
    let request = Request::get("/")
        .header("x-real-ip", "1.2.3.4")
        .body(Body::empty())
        .unwrap();
    let response = router(embed).oneshot(request).await.unwrap();
    let body = response.into_body().collect().await.unwrap().to_bytes();

    assert_eq!(body, "null http http:");
}
