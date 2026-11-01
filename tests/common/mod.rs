#![allow(dead_code)]

use bytes::Bytes;
use openworkers_embed::{HttpMethod, HttpRequest, HttpResponse, RequestBody};
use std::collections::HashMap;

/// Answers with what it received, so a test sees how a request arrived.
pub const ECHO: &str = r#"
export default {
    async fetch(request) {
        const url = new URL(request.url);
        const body = await request.text();

        return new Response(
            `${request.method} ${url.pathname} cookie=${request.headers.get('cookie')} body=${body}`,
            { status: 201, headers: { 'x-echo': 'yes' } },
        );
    },
};
"#;

/// Answers with the client it sees: `x-real-ip`, `x-forwarded-proto` and the
/// scheme of its URL.
pub const CLIENT: &str = r#"
export default {
    async fetch(request) {
        const url = new URL(request.url);

        return new Response(
            `${request.headers.get('x-real-ip')} ${request.headers.get('x-forwarded-proto')} ${url.protocol}`,
        );
    },
};
"#;

pub fn get(path: &str) -> HttpRequest {
    HttpRequest {
        method: HttpMethod::Get,
        url: format!("http://localhost{path}"),
        headers: HashMap::new(),
        body: RequestBody::None,
    }
}

/// The body, read to the end: a worker answers with a stream.
pub async fn body(response: HttpResponse) -> Bytes {
    response
        .body
        .collect()
        .await
        .expect("the body does not fail")
        .unwrap_or_default()
}

pub async fn text(response: HttpResponse) -> String {
    String::from_utf8(body(response).await.to_vec()).expect("a UTF-8 body")
}

pub fn header<'a>(response: &'a HttpResponse, name: &str) -> Option<&'a str> {
    response
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}
