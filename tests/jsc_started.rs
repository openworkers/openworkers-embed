//! The jsc engine fails closed when JavaScriptCore started without polling
//! traps, as a test binary of its own: JSC reads its options once.

#![cfg(feature = "jsc")]

use openworkers_embed::{Embed, EmbedError};

#[tokio::test]
async fn the_build_fails_when_jsc_started_without_polling_traps() {
    // SAFETY: the one test of this binary; no other thread reads the
    // environment yet
    unsafe { std::env::remove_var("JSC_usePollingTraps") };

    // A runtime of the host starts JavaScriptCore before the Embed
    let (runtime, ..) = openworkers_runtime_jsc::Runtime::new();

    drop(runtime);

    let worker = "export default { fetch() { return new Response('ok'); } };";
    let built = Embed::builder(worker).threads(1).build();

    assert!(
        matches!(built, Err(EmbedError::Setting(_))),
        "{:?}",
        built.err()
    );
}
