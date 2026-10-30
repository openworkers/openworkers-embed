use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::HashMap;
use std::fmt::Display;
use std::future::Future;
use std::pin::Pin;

pub(crate) type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

type Command = Box<dyn Fn(Value) -> BoxFuture<Result<Value, String>> + Send + Sync>;

/// The Rust functions a worker calls by name, with JSON in and out.
#[derive(Default)]
pub(crate) struct Commands(HashMap<String, Command>);

impl Commands {
    pub fn register<A, R, E, F, Fut>(&mut self, name: &str, function: F)
    where
        A: DeserializeOwned + Send + 'static,
        R: Serialize,
        E: Display,
        F: Fn(A) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R, E>> + Send + 'static,
    {
        let command: Command = Box::new(move |args| {
            let call = serde_json::from_value::<A>(args)
                .map_err(|error| format!("bad arguments: {error}"))
                .map(&function);

            Box::pin(async move {
                let result = call?.await.map_err(|error| error.to_string())?;

                serde_json::to_value(result).map_err(|error| error.to_string())
            })
        });

        self.0.insert(name.to_string(), command);
    }

    /// `None` if no command has that name.
    pub fn call(&self, name: &str, args: Value) -> Option<BoxFuture<Result<Value, String>>> {
        self.0.get(name).map(|command| command(args))
    }
}
