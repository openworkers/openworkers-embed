//! The `fetch` of a worker goes out over HTTP and its body streams back.
//! nova has no global `fetch`.

#![cfg(all(feature = "fetch", not(feature = "nova")))]

mod common;

use common::{get, text};
use openworkers_embed::Embed;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;

/// A request as the test server reads it: method, path, body.
async fn read_request(stream: &mut TcpStream) -> (String, String, Vec<u8>) {
    let mut buffer = Vec::new();

    let head_end = loop {
        let mut chunk = [0u8; 1024];
        let read = stream.read(&mut chunk).await.unwrap();

        buffer.extend_from_slice(&chunk[..read]);

        if let Some(at) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break at + 4;
        }
    };

    let head = String::from_utf8_lossy(&buffer[..head_end]).to_string();
    let mut lines = head.lines();
    let mut first = lines.next().unwrap().split(' ');
    let method = first.next().unwrap().to_string();
    let path = first.next().unwrap().to_string();
    let length: usize = lines
        .find_map(|line| {
            line.to_lowercase()
                .strip_prefix("content-length: ")?
                .parse()
                .ok()
        })
        .unwrap_or(0);

    while buffer.len() < head_end + length {
        let mut chunk = [0u8; 1024];
        let read = stream.read(&mut chunk).await.unwrap();

        buffer.extend_from_slice(&chunk[..read]);
    }

    (method, path, buffer[head_end..head_end + length].to_vec())
}

async fn answer(mut stream: TcpStream, release: Arc<Notify>) {
    let (method, path, body) = read_request(&mut stream).await;

    match path.as_str() {
        // The second piece waits until the worker has read the first
        "/stream" => {
            let head = "HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n6\r\nhello \r\n";

            stream.write_all(head.as_bytes()).await.unwrap();
            stream.flush().await.unwrap();
            release.notified().await;
            stream.write_all(b"5\r\nworld\r\n0\r\n\r\n").await.unwrap();
        }
        "/redirect" => {
            let head = "HTTP/1.1 302 Found\r\nlocation: /stream\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";

            stream.write_all(head.as_bytes()).await.unwrap();
        }
        _ => {
            let reply = format!("{method} {}", String::from_utf8_lossy(&body));
            let head = format!(
                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
                reply.len()
            );

            stream.write_all(head.as_bytes()).await.unwrap();
        }
    }
}

/// The address of a test server, and the embed whose worker is `code`; the
/// command `first_chunk_seen` lets the second piece of `/stream` go.
async fn setup(code: &str) -> (SocketAddr, Embed) {
    let release = Arc::new(Notify::new());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server_release = release.clone();

    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();

            tokio::spawn(answer(stream, server_release.clone()));
        }
    });

    let embed = Embed::builder(code)
        .threads(1)
        .command_sync("first_chunk_seen", move |_: ()| {
            release.notify_one();
            Ok::<_, String>(())
        })
        .build()
        .unwrap();

    (address, embed)
}

async fn run(code: &str) -> String {
    let (address, embed) = setup(code).await;
    let request = get(&format!("/?addr={address}"));

    let response = tokio::time::timeout(Duration::from_secs(5), embed.fetch(request))
        .await
        .expect("the worker did not answer in 5 seconds")
        .unwrap();

    text(response).await
}

#[tokio::test(flavor = "multi_thread")]
async fn the_body_of_a_response_streams_to_the_worker() {
    // A reply read to the end first would wait for a piece that only comes once
    // the worker has read the first one: the 5 second limit of run() catches it
    let body = run("export default {
            async fetch(request, env) {
                const addr = new URL(request.url).searchParams.get('addr');
                const reader = (await fetch(`http://${addr}/stream`)).body.getReader();
                const decoder = new TextDecoder();
                const first = await reader.read();

                await env.EMBED.invoke('first_chunk_seen');

                const second = await reader.read();

                return new Response(decoder.decode(first.value) + decoder.decode(second.value));
            },
        };")
    .await;

    assert_eq!(body, "hello world");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_method_and_the_body_go_out() {
    let body = run(
        "export default {
            async fetch(request) {
                const addr = new URL(request.url).searchParams.get('addr');
                const response = await fetch(`http://${addr}/echo`, { method: 'POST', body: 'ping' });

                return new Response(await response.text());
            },
        };",
    )
    .await;

    assert_eq!(body, "POST ping");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_redirect_is_the_answer_not_a_hop() {
    let body = run("export default {
            async fetch(request) {
                const addr = new URL(request.url).searchParams.get('addr');
                const response = await fetch(`http://${addr}/redirect`);

                return new Response(`${response.status} ${response.headers.get('location')}`);
            },
        };")
    .await;

    assert_eq!(body, "302 /stream");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_connection_rejects_the_promise() {
    let body = run("export default {
            async fetch() {
                try {
                    await fetch('http://127.0.0.1:1/');

                    return new Response('answered');
                } catch (error) {
                    return new Response('rejected');
                }
            },
        };")
    .await;

    assert_eq!(body, "rejected");
}
