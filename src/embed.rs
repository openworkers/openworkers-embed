use crate::assets::Assets;
use crate::bundle::Bundle;
use crate::commands::Commands;
use crate::host::Host;
use crate::pool::WorkerPool;
use crate::{EmbedError, url};
use openworkers_core::{HttpMethod, HttpRequest, HttpResponse, OperationsHandle, ResponseBody};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fmt::Display;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

pub struct EmbedBuilder {
    code: String,
    commands: Commands,
    assets: Option<Arc<Assets>>,
    threads: usize,
}

impl EmbedBuilder {
    /// Serve the files of a directory (the adapter's `build/assets`) for GET
    /// and HEAD requests, before the worker runs. The worker also reaches them
    /// as `env.ASSETS`.
    pub fn assets(mut self, directory: impl Into<PathBuf>) -> Self {
        self.assets = Some(Arc::new(Assets::new(directory.into())));
        self
    }

    /// Register a Rust function the worker calls with `env.EMBED.invoke(name, args)`.
    /// The arguments arrive deserialized and the result goes back as JSON.
    pub fn command<A, R, E, F, Fut>(mut self, name: &str, function: F) -> Self
    where
        A: DeserializeOwned + Send + 'static,
        R: Serialize,
        E: Display,
        F: Fn(A) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R, E>> + Send + 'static,
    {
        self.commands.register(name, function);
        self
    }

    /// Like [`command`](Self::command), for a function that does not wait.
    pub fn command_sync<A, R, E, F>(self, name: &str, function: F) -> Self
    where
        A: DeserializeOwned + Send + 'static,
        R: Serialize + Send + 'static,
        E: Display + Send + 'static,
        F: Fn(A) -> Result<R, E> + Send + Sync + 'static,
    {
        self.command(name, move |args| std::future::ready(function(args)))
    }

    /// The number of worker threads. The default is one per CPU.
    pub fn threads(mut self, threads: usize) -> Self {
        self.threads = threads.max(1);
        self
    }

    pub fn build(self) -> Result<Embed, EmbedError> {
        let host = Host {
            commands: self.commands,
            assets: self.assets,
        };

        Ok(Embed {
            bundle: Bundle::compile(&self.code, host.bindings())?,
            host: Arc::new(host),
            pool: WorkerPool::start(self.threads)?,
        })
    }
}

/// A worker running inside this process. Hand it requests with [`fetch`](Self::fetch),
/// or mount it in a web framework with one of the [`backend`](crate::backend) modules.
pub struct Embed {
    bundle: Bundle,
    host: Arc<Host>,
    pool: WorkerPool,
}

impl Embed {
    /// `code` is the worker: an ES module with `export default { fetch }`.
    pub fn builder(code: impl Into<String>) -> EmbedBuilder {
        EmbedBuilder {
            code: code.into(),
            commands: Commands::default(),
            assets: None,
            threads: std::thread::available_parallelism().map_or(1, |n| n.get()),
        }
    }

    pub async fn fetch(&self, request: HttpRequest) -> Result<HttpResponse, EmbedError> {
        let path = url::path(&request.url).to_string();

        self.fetch_at(request, &path).await
    }

    /// [`fetch`](Self::fetch), with the path of the request as the server read
    /// it. The URL of the request holds the Host header of the client, which
    /// can hold a `/`, so the path is not taken from it.
    pub(crate) async fn fetch_at(
        &self,
        request: HttpRequest,
        path: &str,
    ) -> Result<HttpResponse, EmbedError> {
        if let Some(response) = self.asset(&request, path).await {
            return Ok(response);
        }

        let ops: OperationsHandle = self.host.clone();

        self.pool.execute(&self.bundle, ops, request).await
    }

    /// The file a GET or HEAD request names, if the assets directory has it.
    async fn asset(&self, request: &HttpRequest, path: &str) -> Option<HttpResponse> {
        let assets = self.host.assets.as_ref()?;

        let head = match request.method {
            HttpMethod::Get => false,
            HttpMethod::Head => true,
            _ => return None,
        };

        let mut response = assets.get(path).await?.into_http();

        if head {
            response.body = ResponseBody::None;
        }

        Some(response)
    }
}
