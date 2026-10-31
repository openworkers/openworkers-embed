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

#[tokio::test]
async fn serve_answers_on_the_worker_threads() {
    let embed = Embed::builder(ECHO).threads(2).build().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();

    tokio::spawn(openworkers_embed::backend::axum::serve(listener, embed));

    let client = reqwest::Client::new();

    // Several requests on one connection, then on new ones
    for _ in 0..4 {
        let response = client
            .post(format!("http://{address}/play"))
            .header("cookie", "a=1")
            .body("paper")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 201);
        assert_eq!(
            response.text().await.unwrap(),
            "POST /play cookie=a=1 body=paper"
        );
    }
}
