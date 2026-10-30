mod common;

use common::{body, get, header, text};
use openworkers_embed::{Embed, EmbedError, HttpMethod};
use std::fs;

const CALLS_A_COMMAND: &str = r#"
export default {
    async fetch(request, env) {
        const name = new URL(request.url).searchParams.get('name');
        const sum = await env.EMBED.invoke('add', { a: 1, b: 2 });
        const greeting = await env.EMBED.invoke('greet', name);

        return new Response(`${sum} ${greeting}`);
    },
};
"#;

fn adder(code: &str) -> openworkers_embed::EmbedBuilder {
    Embed::builder(code)
        .threads(1)
        .command_sync("add", |args: (u32, u32)| Ok::<_, String>(args.0 + args.1))
}

#[derive(serde::Deserialize)]
struct Pair {
    a: u32,
    b: u32,
}

#[tokio::test]
async fn a_worker_calls_rust_commands() {
    let embed = Embed::builder(CALLS_A_COMMAND)
        .threads(1)
        .command_sync("add", |pair: Pair| Ok::<_, String>(pair.a + pair.b))
        .command("greet", |name: String| async move {
            Ok::<_, String>(format!("hello {name}"))
        })
        .build()
        .unwrap();

    let response = embed.fetch(get("/?name=Ada")).await.unwrap();

    assert_eq!(text(response), "3 hello Ada");
}

async fn exception_message(code: &str) -> String {
    let worker = format!(
        "export default {{ async fetch(request, env) {{ try {{ {code} }} catch (error) {{ return new Response(error.message); }} }} }};"
    );
    let embed = adder(&worker).build().unwrap();

    text(embed.fetch(get("/")).await.unwrap())
}

#[tokio::test]
async fn a_failing_command_throws_in_the_worker() {
    let worker = r#"export default { async fetch(request, env) { try { await env.EMBED.invoke('fail'); } catch (error) { return new Response(error.message); } } };"#;
    let embed = Embed::builder(worker)
        .threads(1)
        .command_sync("fail", |_: ()| Err::<(), _>("the disk is full"))
        .build()
        .unwrap();

    assert_eq!(
        text(embed.fetch(get("/")).await.unwrap()),
        "the disk is full"
    );
}

#[tokio::test]
async fn an_unknown_command_throws_in_the_worker() {
    let message = exception_message("await env.EMBED.invoke('nope');").await;

    assert_eq!(message, "no command named 'nope'");
}

#[tokio::test]
async fn bad_arguments_throw_in_the_worker() {
    let message = exception_message("await env.EMBED.invoke('add', 'text');").await;

    assert!(message.starts_with("bad arguments"), "{message}");
}

#[tokio::test]
async fn a_worker_that_does_not_parse_is_refused_at_build() {
    let result = Embed::builder("export default {").build();

    assert!(matches!(result, Err(EmbedError::Code(_))));
}

#[tokio::test]
async fn two_embeds_in_one_process_keep_their_own_code() {
    let first = Embed::builder("export default { fetch: () => new Response('first') };")
        .threads(1)
        .build()
        .unwrap();
    let second = Embed::builder("export default { fetch: () => new Response('second') };")
        .threads(1)
        .build()
        .unwrap();

    for _ in 0..3 {
        assert_eq!(text(first.fetch(get("/")).await.unwrap()), "first");
        assert_eq!(text(second.fetch(get("/")).await.unwrap()), "second");
    }
}

fn site() -> (tempfile::TempDir, Embed) {
    let site = tempfile::tempdir().unwrap();
    let assets = site.path().join("assets");

    fs::create_dir_all(assets.join("_app/immutable")).unwrap();
    fs::write(assets.join("a.txt"), "plain").unwrap();
    fs::write(assets.join("about.html"), "<h1>about</h1>").unwrap();
    fs::write(assets.join("_app/immutable/x.js"), "export {}").unwrap();
    fs::write(site.path().join("secret.txt"), "secret").unwrap();

    let worker = r#"
export default {
    async fetch(request) {
        const path = new URL(request.url).pathname;

        if (path === '/through-env') {
            return env.ASSETS.fetch('/a.txt');
        }

        return new Response('worker');
    },
};
"#;
    let embed = Embed::builder(worker)
        .threads(1)
        .assets(assets)
        .build()
        .unwrap();

    (site, embed)
}

#[tokio::test]
async fn assets_are_served_before_the_worker() {
    let (_site, embed) = site();

    let response = embed.fetch(get("/a.txt")).await.unwrap();

    assert_eq!(response.status, 200);
    assert_eq!(
        header(&response, "content-type"),
        Some("text/plain; charset=utf-8")
    );
    assert_eq!(text(response), "plain");
}

#[tokio::test]
async fn a_prerendered_page_answers_without_its_extension() {
    let (_site, embed) = site();

    assert_eq!(
        text(embed.fetch(get("/about")).await.unwrap()),
        "<h1>about</h1>"
    );
}

#[tokio::test]
async fn hashed_files_are_cached_for_good() {
    let (_site, embed) = site();

    let response = embed.fetch(get("/_app/immutable/x.js")).await.unwrap();

    assert_eq!(
        header(&response, "cache-control"),
        Some("public, max-age=31536000, immutable")
    );
}

#[tokio::test]
async fn head_returns_the_headers_without_the_body() {
    let (_site, embed) = site();
    let mut request = get("/a.txt");

    request.method = HttpMethod::Head;

    let response = embed.fetch(request).await.unwrap();

    assert_eq!(response.status, 200);
    assert!(header(&response, "content-type").is_some());
    assert!(body(response).is_empty());
}

#[tokio::test]
async fn a_missing_file_goes_to_the_worker() {
    let (_site, embed) = site();

    assert_eq!(
        text(embed.fetch(get("/missing.txt")).await.unwrap()),
        "worker"
    );
}

#[tokio::test]
async fn files_outside_the_directory_are_not_served() {
    let (_site, embed) = site();

    let response = embed.fetch(get("/../secret.txt")).await.unwrap();

    assert_eq!(text(response), "worker");
}

// jsc does not serve the Assets binding
#[cfg(not(feature = "jsc"))]
#[tokio::test]
async fn the_worker_reads_assets_through_env() {
    let (_site, embed) = site();

    assert_eq!(
        text(embed.fetch(get("/through-env")).await.unwrap()),
        "plain"
    );
}

#[tokio::test]
async fn the_host_is_on_env_and_not_a_global() {
    let worker = "export default { fetch: (request, env) => new Response(`${typeof embed} ${typeof env.EMBED.invoke}`) };";
    let embed = adder(worker).build().unwrap();

    assert_eq!(
        text(embed.fetch(get("/")).await.unwrap()),
        "undefined function"
    );
}
