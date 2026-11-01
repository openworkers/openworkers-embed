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

/// The headers that only the runner sets (`RUNNER_HEADERS` of
/// openworkers-runner).
pub const RUNNER_HEADERS: [&str; 12] = [
    "x-worker-id",
    "x-worker-name",
    "x-openworkers-internal",
    "x-openworkers-depth",
    "x-request-id",
    "forwarded",
    "x-forwarded-for",
    "x-forwarded-host",
    "x-forwarded-port",
    "x-forwarded-proto",
    "x-real-ip",
    "cf-connecting-ip",
];

/// Answers with the client it sees: its URL, then each header of the runner
/// it got, as `name=value`, one per line.
pub const CLIENT: &str = r#"
const NAMES = [
    'x-worker-id', 'x-worker-name', 'x-openworkers-internal', 'x-openworkers-depth',
    'x-request-id', 'forwarded', 'x-forwarded-for', 'x-forwarded-host',
    'x-forwarded-port', 'x-forwarded-proto', 'x-real-ip', 'cf-connecting-ip',
];

export default {
    async fetch(request) {
        const lines = [request.url];

        for (const name of NAMES) {
            const value = request.headers.get(name);

            if (value !== null) {
                lines.push(`${name}=${value}`);
            }
        }

        return new Response(lines.join('\n'));
    },
};
"#;

/// What [`CLIENT`] answers for a request at `url` from `peer` (or no peer)
/// over `scheme`.
pub fn client_seen(url: &str, peer: Option<&str>, scheme: &str) -> String {
    let mut lines = vec![url.to_string()];

    if let Some(peer) = peer {
        lines.push(format!("x-forwarded-for={peer}"));
    }

    lines.push(format!("x-forwarded-proto={scheme}"));

    if let Some(peer) = peer {
        lines.push(format!("x-real-ip={peer}"));
        lines.push(format!("cf-connecting-ip={peer}"));
    }

    lines.join("\n")
}

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
