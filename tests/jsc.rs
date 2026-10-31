//! The jsc engine: warm workers, reset between requests, under the CPU limit.

#![cfg(feature = "jsc")]

mod common;

use common::{get, text};
use openworkers_core::TerminationReason;
use openworkers_embed::{Embed, EmbedError};
use std::time::{Duration, Instant};

// One thread: the second request runs on the warm worker of the first
#[tokio::test(flavor = "multi_thread")]
async fn a_warm_worker_serves_the_next_request() {
    let worker = "let served = 0;
        export default { fetch() { served++; return new Response(String(served)); } };";
    let embed = Embed::builder(worker).threads(1).build().unwrap();

    assert_eq!(text(embed.fetch(get("/")).await.unwrap()).await, "1");
    assert_eq!(text(embed.fetch(get("/")).await.unwrap()).await, "2");
}

// A request that never yields ends with the CPU limit; the next one gets a new
// worker
#[tokio::test(flavor = "multi_thread")]
async fn a_request_past_the_cpu_limit_is_cut_and_replaced() {
    let worker = "export default {
        fetch(request) {
            if (new URL(request.url).pathname === '/spin') {
                for (;;) {}
            }

            return new Response('ok');
        },
    };";
    let embed = Embed::builder(worker).threads(1).build().unwrap();
    let start = Instant::now();
    let cut = embed.fetch(get("/spin")).await;

    assert!(
        matches!(
            cut,
            Err(EmbedError::Worker(TerminationReason::CpuTimeLimit))
        ),
        "{cut:?}"
    );
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "{:?}",
        start.elapsed()
    );
    assert_eq!(text(embed.fetch(get("/")).await.unwrap()).await, "ok");
}

// Regression test of the top level, not of the warm workers: a top level that
// never ends gives the request an error, at once
#[tokio::test(flavor = "multi_thread")]
async fn a_top_level_that_never_ends_fails_the_request() {
    let worker = "for (;;) {} export default { fetch() { return new Response('ok'); } };";
    let embed = Embed::builder(worker).threads(1).build().unwrap();
    let start = Instant::now();
    let made = embed.fetch(get("/")).await;

    assert!(
        matches!(
            made,
            Err(EmbedError::Worker(TerminationReason::CpuTimeLimit))
        ),
        "{made:?}"
    );
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "{:?}",
        start.elapsed()
    );
}

// Entries of 40 ms chained with setTimeout(0): the budget of the request ends
// it at its second entry, not the wall clock (30 s); the next request gets a
// new worker
#[tokio::test(flavor = "multi_thread")]
async fn a_chain_of_entries_is_cut_by_the_request_budget() {
    let worker = "export default {
        fetch(request) {
            if (new URL(request.url).pathname === '/') {
                return new Response('ok');
            }

            return new Promise((resolve) => {
                const step = () => {
                    const stop = Date.now() + 40;
                    while (Date.now() < stop) {}
                    setTimeout(step, 0);
                };
                setTimeout(step, 0);
            });
        },
    };";
    let embed = Embed::builder(worker).threads(1).build().unwrap();
    let start = Instant::now();
    let cut = embed.fetch(get("/chain")).await;

    assert!(
        matches!(
            cut,
            Err(EmbedError::Worker(TerminationReason::CpuTimeLimit))
        ),
        "{cut:?}"
    );
    assert!(
        start.elapsed() < Duration::from_millis(500),
        "{:?}",
        start.elapsed()
    );
    assert_eq!(text(embed.fetch(get("/")).await.unwrap()).await, "ok");
}

// No init_jsc here: the build of the Embed turns on the polling traps, or
// else a loop that calls native code runs past the budget with the JIT
#[tokio::test(flavor = "multi_thread")]
async fn a_loop_of_native_calls_is_cut_without_an_explicit_init() {
    let worker = "export default { fetch() { for (;;) { Date.now(); } } };";
    let embed = Embed::builder(worker).threads(1).build().unwrap();
    let start = Instant::now();
    let cut = embed.fetch(get("/")).await;

    assert!(
        matches!(
            cut,
            Err(EmbedError::Worker(TerminationReason::CpuTimeLimit))
        ),
        "{cut:?}"
    );
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "{:?}",
        start.elapsed()
    );
}
