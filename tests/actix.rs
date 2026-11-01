#![cfg(feature = "actix")]

mod common;

use actix_web::{App, test, web};
use common::{CLIENT, ECHO, RUNNER_HEADERS, client_seen};
use openworkers_embed::Embed;
use openworkers_embed::backend::actix::handle;

#[actix_web::test]
async fn an_actix_app_sends_requests_to_the_worker() {
    let embed = Embed::builder(ECHO).threads(1).build().unwrap();

    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(embed))
            .default_service(web::to(handle)),
    )
    .await;

    let request = test::TestRequest::post()
        .uri("/play")
        .insert_header(("cookie", "a=1"))
        .set_payload("scissors")
        .to_request();

    let response = test::call_service(&app, request).await;

    assert_eq!(response.status(), 201);
    assert_eq!(response.headers().get("x-echo").unwrap(), "yes");

    let body = test::read_body(response).await;

    assert_eq!(body, "POST /play cookie=a=1 body=scissors");
}

// The client sends every header of the runner, as if it were 1.2.3.4 over
// https: the worker sees only the peer and the scheme of the server
#[actix_web::test]
async fn the_worker_sees_the_peer_and_not_the_headers_of_the_client() {
    let embed = Embed::builder(CLIENT).threads(1).build().unwrap();

    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(embed))
            .default_service(web::to(handle)),
    )
    .await;

    let mut request = test::TestRequest::get()
        .uri("/x")
        .insert_header(("host", "app.test"))
        .peer_addr("192.0.2.7:4000".parse().unwrap());

    for name in RUNNER_HEADERS {
        request = request.insert_header((name, "1.2.3.4"));
    }

    let request = request
        .insert_header(("x-forwarded-proto", "https"))
        .to_request();

    let body = test::read_body(test::call_service(&app, request).await).await;

    assert_eq!(
        body,
        client_seen("http://app.test/x", Some("192.0.2.7"), "http")
    );
}

// Forwarded and X-Forwarded-Host of the client do not choose the URL of the
// worker: actix's connection_info would read them
#[actix_web::test]
async fn the_url_comes_from_the_host_header_and_the_request_line() {
    let embed = Embed::builder(CLIENT).threads(1).build().unwrap();

    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(embed))
            .default_service(web::to(handle)),
    )
    .await;

    let request = test::TestRequest::get()
        .uri("/page?q=1")
        .insert_header(("host", "app.test"))
        .insert_header((
            "forwarded",
            "proto=http://evil.example/admin?x=;host=evil.example",
        ))
        .insert_header(("x-forwarded-host", "evil.example"))
        .to_request();

    let body = test::read_body(test::call_service(&app, request).await).await;

    assert_eq!(body, client_seen("http://app.test/page?q=1", None, "http"));
}
