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
