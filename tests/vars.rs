mod common;

use common::{get, text};
use openworkers_embed::{Embed, EmbedError};

/// As the Astro adapter does: a page reads `env` of `fetch`, and `astro:env`
/// reads `globalThis.env` through `setGetEnv`.
const ASTRO_STYLE: &str = r#"
const getEnv = (key) => globalThis.env?.[key];

export default {
    async fetch(request, env) {
        return new Response(`${env.GREETING} ${getEnv('SECRET')} ${env.MISSING}`);
    },
};
"#;

#[tokio::test]
async fn a_worker_reads_its_variables_from_env_and_from_globalthis() {
    let embed = Embed::builder(ASTRO_STYLE)
        .threads(1)
        .var("GREETING", "hello")
        .var("SECRET", "s3cret")
        .build()
        .unwrap();

    let response = embed.fetch(get("/")).await.unwrap();

    assert_eq!(text(response).await, "hello s3cret undefined");
}

#[tokio::test]
async fn a_second_value_replaces_the_first() {
    let embed = Embed::builder(ASTRO_STYLE)
        .threads(1)
        .var("GREETING", "hello")
        .var("GREETING", "bonjour")
        .build()
        .unwrap();

    let response = embed.fetch(get("/")).await.unwrap();

    assert_eq!(text(response).await, "bonjour undefined undefined");
}

#[test]
fn a_variable_cannot_take_the_name_of_a_binding() {
    for name in ["EMBED", "ASSETS"] {
        let built = Embed::builder(ASTRO_STYLE).var(name, "x").build();

        assert!(
            matches!(&built, Err(EmbedError::Setting(message)) if message.contains(name)),
            "{name}"
        );
    }
}
