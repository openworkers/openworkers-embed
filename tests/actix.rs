#![cfg(feature = "actix")]

mod common;

use actix_web::{App, test, web};
use common::ECHO;
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
