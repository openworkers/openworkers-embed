#![allow(dead_code)]

use bytes::Bytes;
use openworkers_embed::{HttpMethod, HttpRequest, HttpResponse, RequestBody, ResponseBody};
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

pub fn get(path: &str) -> HttpRequest {
    HttpRequest {
        method: HttpMethod::Get,
        url: format!("http://localhost{path}"),
        headers: HashMap::new(),
        body: RequestBody::None,
    }
}

pub fn body(response: HttpResponse) -> Bytes {
    match response.body {
        ResponseBody::Bytes(bytes) => bytes,
        ResponseBody::None => Bytes::new(),
        ResponseBody::Stream(_) => panic!("the body streams"),
    }
}

pub fn text(response: HttpResponse) -> String {
    String::from_utf8(body(response).to_vec()).expect("a UTF-8 body")
}

pub fn header<'a>(response: &'a HttpResponse, name: &str) -> Option<&'a str> {
    response
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}
