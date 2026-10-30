//! The outbound `fetch` of a worker, with the client settings of the runner but
//! not its egress filter: the code is trusted, and may call a service on
//! this machine.

use futures_util::StreamExt;
use openworkers_core::{HttpMethod, HttpRequest, HttpResponse, ResponseBody};
use std::time::Duration;
use tokio::sync::mpsc;

/// Chunks that wait for the guest to read them: the pump stops while it is full.
const BODY_BUFFER: usize = 16;

thread_local! {
    // One client per worker thread, because its connections live on the runtime
    // of that thread
    static CLIENT: reqwest::Client = reqwest::Client::builder()
        .user_agent(concat!("openworkers-embed/", env!("CARGO_PKG_VERSION")))
        .pool_max_idle_per_host(100)
        .pool_idle_timeout(Duration::from_secs(90))
        .connect_timeout(Duration::from_secs(5))
        // A stall cap, not a budget: a body that keeps arriving takes as long as it takes
        .read_timeout(Duration::from_secs(10))
        // The guest follows redirects itself
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("HTTP client");
}

pub(crate) async fn fetch(request: HttpRequest) -> Result<HttpResponse, String> {
    let method = match request.method {
        HttpMethod::Get => reqwest::Method::GET,
        HttpMethod::Post => reqwest::Method::POST,
        HttpMethod::Put => reqwest::Method::PUT,
        HttpMethod::Delete => reqwest::Method::DELETE,
        HttpMethod::Patch => reqwest::Method::PATCH,
        HttpMethod::Head => reqwest::Method::HEAD,
        HttpMethod::Options => reqwest::Method::OPTIONS,
    };

    let mut builder = CLIENT.with(Clone::clone).request(method, &request.url);

    for (name, value) in &request.headers {
        builder = builder.header(name, value);
    }

    if let Some(body) = request.body.collect().await? {
        builder = builder.body(body);
    }

    let response = builder
        .send()
        .await
        .map_err(|error| format!("Request failed: {error}"))?;

    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| Some((name.to_string(), value.to_str().ok()?.to_string())))
        .collect();

    let (sender, body) = mpsc::channel(BODY_BUFFER);

    // spawn, not spawn_local: this runs on the runtime of a worker thread, which
    // already has a LocalSet, and the pump needs nothing local
    tokio::spawn(async move {
        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| error.to_string());
            let failed = chunk.is_err();

            if sender.send(chunk).await.is_err() || failed {
                break;
            }
        }
    });

    Ok(HttpResponse {
        status,
        headers,
        body: ResponseBody::Stream(body),
    })
}
