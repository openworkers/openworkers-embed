#![cfg(feature = "axum")]

mod common;

use axum::body::Body;
use axum::http::Request;
use common::ECHO;
use http_body_util::BodyExt;
use openworkers_embed::Embed;
use openworkers_embed::backend::axum::router;
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
