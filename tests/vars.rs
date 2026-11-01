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
fn a_variable_name_is_an_identifier() {
    for name in ["", " ", "MY VAR", "1ST", "a-b", "__proto__", "é"] {
        let built = Embed::builder(ASTRO_STYLE).var(name, "x").build();

        assert!(
            matches!(&built, Err(EmbedError::Setting(message)) if message.contains("identifier")),
            "{name:?}"
        );
    }

    for name in ["A", "_a", "$a", "API_KEY_2", "proto"] {
        assert!(
            Embed::builder(ASTRO_STYLE).var(name, "x").build().is_ok(),
            "{name:?}"
        );
    }
}

// The variables do not hide the binding of the commands
#[tokio::test]
async fn commands_still_work_with_variables() {
    let worker = r#"
export default {
    async fetch(request, env) {
        const sum = await env.EMBED.invoke('add', [env.A, 2]);

        return new Response(`${sum} ${env.A}`);
    },
};
"#;
    let embed = Embed::builder(worker)
        .threads(1)
        .var("A", "1")
        .command_sync("add", |(a, b): (String, u32)| {
            Ok::<_, String>(a.parse::<u32>().unwrap() + b)
        })
        .build()
        .unwrap();

    let response = embed.fetch(get("/")).await.unwrap();

    assert_eq!(text(response).await, "3 1");
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
