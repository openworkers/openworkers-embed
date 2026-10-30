use crate::assets::Assets;
use crate::commands::Commands;
use crate::response::text;
use crate::url;
use openworkers_core::{
    BindingInfo, BindingType, HttpRequest, HttpResponse, InvokeResult, OpFuture, OperationsHandler,
};
use std::sync::Arc;

/// The binding that carries every command; the shim wraps it in `embed.invoke`.
const COMMAND_BINDING: &str = "__embed";

/// The name the adapter's worker reads static files from.
const ASSETS_BINDING: &str = "ASSETS";

/// What a worker reaches on the Rust side: commands, and the assets directory.
pub(crate) struct Host {
    pub commands: Commands,
    pub assets: Option<Arc<Assets>>,
}

impl Host {
    pub fn bindings(&self) -> Vec<BindingInfo> {
        let mut bindings = vec![BindingInfo::invoke(COMMAND_BINDING)];

        if self.assets.is_some() {
            bindings.push(BindingInfo::new(ASSETS_BINDING, BindingType::Assets));
        }

        bindings
    }
}

impl OperationsHandler for Host {
    /// The adapter's worker calls `env.ASSETS.fetch` for prerendered pages and
    /// for files it expects to find.
    fn handle_binding_fetch(
        &self,
        binding: &str,
        request: HttpRequest,
    ) -> OpFuture<'_, Result<HttpResponse, String>> {
        let name = binding.to_string();
        let assets = self.assets.clone().filter(|_| name == ASSETS_BINDING);

        Box::pin(async move {
            let Some(assets) = assets else {
                return Err(format!("Binding '{name}' not configured"));
            };

            Ok(match assets.get(url::path(&request.url)).await {
                Some(asset) => asset.into_http(),
                None => text(404, "not found"),
            })
        })
    }

    /// `embed.invoke(name, args)`: runs the command and answers its result.
    fn handle_binding_invoke(
        &self,
        binding: &str,
        name: &str,
        args: serde_json::Value,
    ) -> OpFuture<'_, InvokeResult> {
        let call = (binding == COMMAND_BINDING)
            .then(|| self.commands.call(name, args))
            .flatten();
        let unknown = format!("no command named '{name}'");

        Box::pin(async move {
            let Some(call) = call else {
                return InvokeResult::Error(unknown);
            };

            match call.await {
                Ok(value) => InvokeResult::Value(value),
                Err(message) => InvokeResult::Error(message),
            }
        })
    }
}
