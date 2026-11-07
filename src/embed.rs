use crate::assets::Assets;
use crate::bundle::Bundle;
use crate::commands::Commands;
use crate::host::{BINDINGS, Host};
use crate::pool::WorkerPool;
use crate::{EmbedError, url};
use openworkers_core::{HttpMethod, HttpRequest, HttpResponse, OperationsHandle, ResponseBody};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::fmt::Display;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

pub struct EmbedBuilder {
    code: String,
    commands: Commands,
    assets: Option<Arc<Assets>>,
    #[cfg(feature = "fetch")]
    fetch: bool,
    vars: HashMap<String, String>,
    threads: usize,
    cpu_limit_ms: Option<u64>,
}

impl EmbedBuilder {
    /// Serve the files of a directory (the adapter's `build/assets`) for GET
    /// and HEAD requests, before the worker runs. The worker also reaches them
    /// as `env.ASSETS`. Only the files found at start are served.
    pub fn assets(mut self, directory: impl Into<PathBuf>) -> Self {
        self.assets = Some(Arc::new(Assets::new(directory.into())));
        self
    }

    /// Like [`assets`](Self::assets), and the files that a rebuild adds after
    /// start are served too. For development.
    pub fn assets_reloading(mut self, directory: impl Into<PathBuf>) -> Self {
        self.assets = Some(Arc::new(Assets::reloading(directory.into())));
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
    ///
    /// The function runs inside the turn of the worker's JavaScript, so its
    /// time counts in the CPU limit of the request: 50 ms by default. A
    /// request whose functions and JavaScript pass it ends with
    /// `TerminationReason::CpuTimeLimit`.
    pub fn command_sync<A, R, E, F>(self, name: &str, function: F) -> Self
    where
        A: DeserializeOwned + Send + 'static,
        R: Serialize + Send + 'static,
        E: Display + Send + 'static,
        F: Fn(A) -> Result<R, E> + Send + Sync + 'static,
    {
        self.command(name, move |args| std::future::ready(function(args)))
    }

    /// Whether the `fetch` of the worker goes out over HTTP. It is on by default.
    /// `fetch(false)` makes the call throw `Fetch not available`; the assets and
    /// the commands do not use it.
    ///
    /// Without the `fetch` feature, `fetch(true)` panics.
    pub fn fetch(self, enabled: bool) -> Self {
        #[cfg(feature = "fetch")]
        return Self {
            fetch: enabled,
            ..self
        };

        #[cfg(not(feature = "fetch"))]
        {
            assert!(!enabled, "fetch(true) needs the `fetch` feature");
            self
        }
    }

    /// A text variable: the worker reads it as `env.<name>`, in the `env` of
    /// `fetch` and in `globalThis.env` (where `astro:env` looks). A second call
    /// with the same name replaces the value. [`build`](Self::build) refuses a
    /// name that is not a JavaScript identifier (letters, digits, `_` and `$`,
    /// not starting with a digit), `__proto__`, and the names of the bindings,
    /// `EMBED` and `ASSETS`.
    pub fn var(mut self, name: &str, value: &str) -> Self {
        self.vars.insert(name.to_string(), value.to_string());
        self
    }

    /// The number of worker threads. The default is one per CPU.
    pub fn threads(mut self, threads: usize) -> Self {
        self.threads = threads.max(1);
        self
    }

    /// The CPU time one request may use, in milliseconds: 50 by default, 0 turns
    /// the limit off. The time of every turn of the request counts, from one
    /// budget: the JavaScript, the commands that do not wait, and the first
    /// compile of the worker code in a place that has not run it yet. Waiting
    /// does not count. Only the v8 engine applies it.
    pub fn cpu_limit(mut self, milliseconds: u64) -> Self {
        self.cpu_limit_ms = Some(milliseconds);
        self
    }

    pub fn build(self) -> Result<Embed, EmbedError> {
        let host = Host {
            commands: self.commands,
            assets: self.assets,
            #[cfg(feature = "fetch")]
            fetch: self.fetch,
        };

        for name in self.vars.keys() {
            check_var(name)?;
        }

        Ok(Embed {
            bundle: Bundle::compile(&self.code, self.vars, host.bindings())?
                .with_cpu_limit(self.cpu_limit_ms),
            host: Arc::new(host),
            pool: WorkerPool::start(self.threads)?,
        })
    }
}

/// A name the worker reads as `env.<name>`: an identifier, which the engines
/// would not drop. `__proto__` sets the prototype of `env` instead of a key.
fn check_var(name: &str) -> Result<(), EmbedError> {
    let mut chars = name.chars();
    let identifier = chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_' || first == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$');

    if !identifier || name == "__proto__" {
        return Err(EmbedError::Setting(format!(
            "the variable name {name:?} is not an identifier the worker can read"
        )));
    }

    if BINDINGS.contains(&name) {
        return Err(EmbedError::Setting(format!(
            "the variable {name} has the name of a binding"
        )));
    }

    Ok(())
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
            #[cfg(feature = "fetch")]
            fetch: true,
            vars: HashMap::new(),
            threads: std::thread::available_parallelism().map_or(1, |n| n.get()),
            cpu_limit_ms: None,
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

    #[cfg(all(test, feature = "v8"))]
    pub(crate) fn pool(&self) -> &WorkerPool {
        &self.pool
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
