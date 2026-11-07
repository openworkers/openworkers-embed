mod common;

use common::{body, get, header, text};
use openworkers_embed::{Embed, EmbedError, HttpMethod};
use std::fs;
use std::time::Duration;

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

    assert_eq!(text(response).await, "3 hello Ada");
}

async fn exception_message(code: &str) -> String {
    let worker = format!(
        "export default {{ async fetch(request, env) {{ try {{ {code} }} catch (error) {{ return new Response(error.message); }} }} }};"
    );
    let embed = adder(&worker).build().unwrap();

    text(embed.fetch(get("/")).await.unwrap()).await
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
        text(embed.fetch(get("/")).await.unwrap()).await,
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
        assert_eq!(text(first.fetch(get("/")).await.unwrap()).await, "first");
        assert_eq!(text(second.fetch(get("/")).await.unwrap()).await, "second");
    }
}

fn site() -> (tempfile::TempDir, Embed) {
    let site = tempfile::tempdir().unwrap();
    let assets = site.path().join("assets");

    fs::create_dir_all(assets.join("_app/immutable")).unwrap();
    fs::create_dir_all(assets.join("_astro")).unwrap();
    fs::write(assets.join("a.txt"), "plain").unwrap();
    fs::write(assets.join("about.html"), "<h1>about</h1>").unwrap();
    fs::write(assets.join("_app/immutable/x.js"), "export {}").unwrap();
    fs::write(assets.join("_astro/page.Dx1.js"), "export {}").unwrap();
    fs::create_dir_all(assets.join("a/b/_astro")).unwrap();
    fs::write(assets.join("a/b/_astro/x.js"), "export {}").unwrap();
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
    assert_eq!(text(response).await, "plain");
}

#[tokio::test]
async fn a_prerendered_page_answers_without_its_extension() {
    let (_site, embed) = site();

    assert_eq!(
        text(embed.fetch(get("/about")).await.unwrap()).await,
        "<h1>about</h1>"
    );
}

#[tokio::test]
async fn hashed_files_are_cached_for_good() {
    let (_site, embed) = site();

    // SvelteKit, then Astro
    for path in ["/_app/immutable/x.js", "/_astro/page.Dx1.js"] {
        let response = embed.fetch(get(path)).await.unwrap();

        assert_eq!(
            header(&response, "cache-control"),
            Some("public, max-age=31536000, immutable"),
            "{path}"
        );
    }

    // A plain file, and an _astro directory deeper than the root
    for path in ["/a.txt", "/a/b/_astro/x.js"] {
        let response = embed.fetch(get(path)).await.unwrap();

        assert_eq!(response.status, 200, "{path}");
        assert_eq!(header(&response, "cache-control"), None, "{path}");
    }
}

#[tokio::test]
async fn head_returns_the_headers_without_the_body() {
    let (_site, embed) = site();
    let mut request = get("/a.txt");

    request.method = HttpMethod::Head;

    let response = embed.fetch(request).await.unwrap();

    assert_eq!(response.status, 200);
    assert!(header(&response, "content-type").is_some());
    assert!(body(response).await.is_empty());
}

#[tokio::test]
async fn a_missing_file_goes_to_the_worker() {
    let (_site, embed) = site();

    assert_eq!(
        text(embed.fetch(get("/missing.txt")).await.unwrap()).await,
        "worker"
    );
}

#[tokio::test]
async fn files_outside_the_directory_are_not_served() {
    let (_site, embed) = site();

    let response = embed.fetch(get("/../secret.txt")).await.unwrap();

    assert_eq!(text(response).await, "worker");
}

// jsc does not serve the Assets binding
#[cfg(not(feature = "jsc"))]
#[tokio::test]
async fn the_worker_reads_assets_through_env() {
    let (_site, embed) = site();

    assert_eq!(
        text(embed.fetch(get("/through-env")).await.unwrap()).await,
        "plain"
    );
}

#[tokio::test]
async fn the_host_is_on_env_and_not_a_global() {
    let worker = "export default { fetch: (request, env) => new Response(`${typeof embed} ${typeof env.EMBED.invoke}`) };";
    let embed = adder(worker).build().unwrap();

    assert_eq!(
        text(embed.fetch(get("/")).await.unwrap()).await,
        "undefined function"
    );
}

// A worker answers with a stream: the response is handed over as soon as the
// worker has one, and the rest comes while the worker keeps running.

// nova and boa drain a stream before they answer
#[cfg(not(any(feature = "nova", feature = "boa")))]
#[tokio::test(flavor = "multi_thread")]
async fn a_response_streams_out_while_the_worker_runs() {
    let release = std::sync::Arc::new(tokio::sync::Notify::new());
    let worker = "export default {
        fetch(request, env) {
            const encoder = new TextEncoder();
            let step = 0;

            return new Response(new ReadableStream({
                async pull(controller) {
                    if (step++ === 0) {
                        controller.enqueue(encoder.encode('first '));

                        return;
                    }

                    await env.EMBED.invoke('release');
                    controller.enqueue(encoder.encode('second'));
                    controller.close();
                },
            }));
        },
    };";
    let waiting = release.clone();
    let embed = Embed::builder(worker)
        .threads(1)
        .command("release", move |_: ()| {
            let release = waiting.clone();

            async move {
                release.notified().await;
                Ok::<_, String>(())
            }
        })
        .build()
        .unwrap();

    // A response read to the end first would wait for a second piece that comes
    // only after the test has seen the first
    let response = tokio::time::timeout(Duration::from_secs(5), embed.fetch(get("/")))
        .await
        .expect("the response did not come while the worker was running")
        .unwrap();
    let openworkers_embed::ResponseBody::Stream(mut chunks) = response.body else {
        panic!("the body is a stream");
    };

    assert_eq!(chunks.recv().await.unwrap().unwrap(), "first ");

    release.notify_one();

    let mut rest = Vec::new();

    while let Some(chunk) = tokio::time::timeout(Duration::from_secs(5), chunks.recv())
        .await
        .expect("the stream did not end")
    {
        rest.extend_from_slice(&chunk.unwrap());
    }

    assert_eq!(rest, b"second");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_long_stream_arrives_whole() {
    // The CPU limit bounds the stream; keep the pulls under it in debug
    let worker = "export default {
        fetch() {
            let n = 0;

            return new Response(new ReadableStream({
                pull(controller) {
                    if (n++ < 500) {
                        controller.enqueue(new TextEncoder().encode('x'));
                    } else {
                        controller.close();
                    }
                },
            }));
        },
    };";
    let embed = Embed::builder(worker).threads(1).build().unwrap();
    let response = embed.fetch(get("/")).await.unwrap();

    let body = tokio::time::timeout(Duration::from_secs(10), text(response))
        .await
        .expect("the stream did not end");

    assert_eq!(body.len(), 500);
}

// nova and boa drain a stream before they answer, and an endless one never
// ends
#[cfg(not(any(feature = "nova", feature = "boa")))]
#[tokio::test(flavor = "multi_thread")]
async fn a_dropped_response_does_not_block_the_next_request() {
    let worker = "export default {
        fetch(request) {
            if (new URL(request.url).pathname === '/hello') {
                return new Response('hello');
            }

            return new Response(new ReadableStream({
                pull(controller) {
                    controller.enqueue(new TextEncoder().encode('x'));
                },
            }));
        },
    };";
    let embed = Embed::builder(worker).threads(1).build().unwrap();

    drop(embed.fetch(get("/endless")).await.unwrap());

    let hello = tokio::time::timeout(Duration::from_secs(5), embed.fetch(get("/hello")))
        .await
        .expect("the next request did not come")
        .unwrap();

    assert_eq!(text(hello).await, "hello");
}

// Only v8 enforces a CPU limit
#[cfg(feature = "v8")]
#[tokio::test(flavor = "multi_thread")]
async fn a_worker_the_runtime_stops_ends_its_stream_with_an_error() {
    let worker = "export default {
        fetch() {
            let step = 0;

            return new Response(new ReadableStream({
                async pull(controller) {
                    if (step++ === 0) {
                        controller.enqueue(new TextEncoder().encode('first '));

                        return;
                    }

                    // The wait is not CPU time: the response is out when the burn starts
                    await new Promise((resolve) => setTimeout(resolve, 200));

                    const until = Date.now() + 500;

                    while (Date.now() < until);
                },
            }));
        },
    };";
    let embed = Embed::builder(worker).threads(1).build().unwrap();
    let response = embed.fetch(get("/")).await.unwrap();
    let openworkers_embed::ResponseBody::Stream(mut chunks) = response.body else {
        panic!("the body is a stream");
    };

    let mut seen = Vec::new();

    while let Some(item) = tokio::time::timeout(Duration::from_secs(10), chunks.recv())
        .await
        .expect("the stream stayed open")
    {
        seen.push(item);
    }

    // The worker spends more CPU than its limit allows. Left alone, the stream
    // would stay open and the client would wait; it ends with an error
    assert!(matches!(seen.last(), Some(Err(_))), "{seen:?}");
}

// Only v8 has a pool of isolates with a limited number of places
#[cfg(feature = "v8")]
#[tokio::test(flavor = "multi_thread")]
async fn requests_past_the_places_of_a_thread_wait_for_one() {
    let worker = "export default {
        async fetch() {
            await new Promise((resolve) => setTimeout(resolve, 50));

            return new Response('ok');
        },
    };";
    let embed = std::sync::Arc::new(Embed::builder(worker).threads(1).build().unwrap());
    let mut calls = tokio::task::JoinSet::new();

    for _ in 0..40 {
        let embed = embed.clone();

        calls.spawn(async move { text(embed.fetch(get("/")).await.unwrap()).await });
    }

    let all = tokio::time::timeout(Duration::from_secs(20), async {
        while let Some(answer) = calls.join_next().await {
            assert_eq!(answer.unwrap(), "ok");
        }
    });

    all.await.expect("a request never got a place");
}

/// A worker with routes: /stream streams for 2 s, a piece every 10 ms; /busy
/// calls a command that is always ready, for 1 s; /spin calls for 1 s a
/// command that works 1 ms in Rust; / answers at once.
#[cfg(feature = "v8")]
const TWO_ROUTES: &str = "export default {
    async fetch(request, env) {
        const path = new URL(request.url).pathname;

        if (path === '/stream') {
            const encoder = new TextEncoder();
            const end = Date.now() + 2000;

            return new Response(new ReadableStream({
                async pull(controller) {
                    if (Date.now() > end) {
                        controller.close();

                        return;
                    }

                    await new Promise((resolve) => setTimeout(resolve, 10));
                    controller.enqueue(encoder.encode('.'));
                },
            }));
        }

        if (path === '/spin') {
            const end = Date.now() + 1000;

            while (Date.now() < end) {
                await env.EMBED.invoke('spin');
            }

            return new Response('spun');
        }

        if (path === '/busy') {
            const end = Date.now() + 1000;
            let calls = 0;

            while (Date.now() < end) {
                calls += await env.EMBED.invoke('one');
            }

            return new Response(String(calls));
        }

        return new Response('ok');
    },
};";

#[cfg(feature = "v8")]
fn two_routes() -> std::sync::Arc<Embed> {
    std::sync::Arc::new(
        Embed::builder(TWO_ROUTES)
            .threads(1)
            .command_sync("one", |_: ()| Ok::<_, String>(1))
            .command_sync("spin", |_: ()| {
                let end = std::time::Instant::now() + Duration::from_millis(1);

                while std::time::Instant::now() < end {}

                Ok::<_, String>(())
            })
            .build()
            .unwrap(),
    )
}

/// The time a request to / takes while a request to `path` runs on the thread.
#[cfg(feature = "v8")]
async fn wait_behind(path: &'static str) -> Duration {
    let embed = two_routes();

    // Two places, warm, so the build of an isolate does not count
    let (a, b) = tokio::join!(embed.fetch(get("/")), embed.fetch(get("/")));
    text(a.unwrap()).await;
    text(b.unwrap()).await;

    let first = {
        let embed = embed.clone();

        // /busy may end with the CPU limit, which also bounds the wait
        tokio::spawn(async move {
            if let Ok(response) = embed.fetch(get(path)).await {
                response.body.collect().await.ok();
            }
        })
    };

    tokio::time::sleep(Duration::from_millis(20)).await;

    let start = std::time::Instant::now();
    let answer = text(embed.fetch(get("/")).await.unwrap()).await;
    let waited = start.elapsed();

    assert_eq!(answer, "ok");
    first.await.unwrap();

    waited
}

// A request that streams its body still runs, but the thread parks between
// two pieces, so a new request starts then, not at the end of the stream
#[cfg(feature = "v8")]
#[tokio::test(flavor = "multi_thread")]
async fn a_request_starts_while_another_one_streams() {
    let waited = wait_behind("/stream").await;

    eprintln!("waited behind /stream: {waited:?}");
    assert!(waited < Duration::from_millis(300), "{waited:?}");
}

// A request that never lets the thread park does not hold a new request back
// for long: the CPU limit of 50 ms ends it
#[cfg(feature = "v8")]
#[tokio::test(flavor = "multi_thread")]
async fn a_request_starts_while_another_one_never_parks() {
    let waited = wait_behind("/busy").await;

    eprintln!("waited behind /busy: {waited:?}");
    assert!(waited < Duration::from_millis(300), "{waited:?}");
}

// A request that calls a slow Rust command again and again does not hold a new
// request back for long: MAX_HOLD starts it, and the CPU limit, which counts a
// command that answers at once, ends the slow request
#[cfg(feature = "v8")]
#[tokio::test(flavor = "multi_thread")]
async fn a_request_starts_while_another_one_works_in_rust() {
    let waited = wait_behind("/spin").await;

    eprintln!("waited behind /spin: {waited:?}");
    assert!(waited < Duration::from_millis(300), "{waited:?}");
}

// A worker that never yields ends with the CPU limit, on Linux and on macOS
#[cfg(feature = "v8")]
#[tokio::test(flavor = "multi_thread")]
async fn a_worker_that_never_yields_ends_with_the_cpu_limit() {
    let embed = Embed::builder("export default { fetch() { while (true) {} } };")
        .threads(1)
        .build()
        .unwrap();
    let start = std::time::Instant::now();
    let answer = tokio::time::timeout(Duration::from_secs(5), embed.fetch(get("/")))
        .await
        .expect("the request was not cut");

    assert!(
        matches!(
            answer,
            Err(EmbedError::Worker(
                openworkers_core::TerminationReason::CpuTimeLimit
            ))
        ),
        "{answer:?}"
    );
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "{:?}",
        start.elapsed()
    );
}

// A worker that tries to replace the timer map of its place: the runtime
// keeps the map read-only and resets with what it took before guest code, so
// the next requests keep their timers
#[cfg(feature = "v8")]
#[tokio::test(flavor = "multi_thread")]
async fn a_worker_cannot_break_the_timers_of_its_place() {
    let worker = "export default {
        async fetch(request) {
            if (new URL(request.url).pathname === '/break') {
                globalThis.__timerCallbacks = null;

                return new Response('ok');
            }

            await new Promise((resolve) => setTimeout(resolve, 1));

            return new Response('ok');
        },
    };";
    let embed = Embed::builder(worker).threads(1).build().unwrap();

    assert_eq!(text(embed.fetch(get("/break")).await.unwrap()).await, "ok");
    assert_eq!(text(embed.fetch(get("/")).await.unwrap()).await, "ok");
    assert_eq!(text(embed.fetch(get("/")).await.unwrap()).await, "ok");
}

// A stream whose reader does not read: the pieces made stay bounded and stop
// growing, and the thread parks, so another request runs meanwhile
#[cfg(feature = "v8")]
#[tokio::test(flavor = "multi_thread")]
async fn a_stream_that_waits_for_its_reader_is_bounded_and_lets_others_run() {
    let worker = "export default {
        fetch(request, env) {
            if (new URL(request.url).pathname !== '/stream') {
                return new Response('ok');
            }

            const piece = new TextEncoder().encode('x');

            return new Response(new ReadableStream({
                pull(controller) {
                    env.EMBED.invoke('count');
                    controller.enqueue(piece);
                },
            }));
        },
    };";
    let made = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = made.clone();
    let embed = Embed::builder(worker)
        .threads(1)
        .command_sync("count", move |_: ()| {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok::<_, String>(())
        })
        .build()
        .unwrap();

    // Three channels of 16 pieces stand between the pump and the reader
    let stream = embed.fetch(get("/stream")).await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let first = made.load(std::sync::atomic::Ordering::SeqCst);

    tokio::time::sleep(Duration::from_millis(200)).await;

    let second = made.load(std::sync::atomic::Ordering::SeqCst);

    assert!(first <= 64, "{first} pieces made with no reader");
    assert_eq!(first, second, "the pump went on with no room");

    let start = std::time::Instant::now();

    assert_eq!(text(embed.fetch(get("/")).await.unwrap()).await, "ok");
    assert!(
        start.elapsed() < Duration::from_millis(300),
        "{:?}",
        start.elapsed()
    );

    drop(stream);
}

#[tokio::test]
async fn a_worker_cannot_fetch_when_fetch_is_off() {
    let worker = "export default { async fetch() { try { await fetch('http://127.0.0.1:9/'); return new Response('reached'); } catch (error) { return new Response(error.message); } } };";
    let embed = Embed::builder(worker)
        .threads(1)
        .fetch(false)
        .build()
        .unwrap();
    let message = text(embed.fetch(get("/")).await.unwrap()).await;

    assert!(message.contains("not available"), "{message}");
}

#[cfg(not(feature = "fetch"))]
#[test]
#[should_panic(expected = "needs the `fetch` feature")]
fn fetch_needs_its_feature() {
    Embed::builder("export default { fetch: () => new Response('') };").fetch(true);
}
