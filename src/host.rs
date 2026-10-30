use crate::assets::Assets;
use crate::commands::Commands;
use crate::response::{reply, text};
use crate::url;
use openworkers_core::{
    BindingInfo, BindingType, HttpRequest, HttpResponse, OpFuture, OperationsHandler,
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
        let mut bindings = vec![BindingInfo::new(COMMAND_BINDING, BindingType::Worker)];

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

    /// `embed.invoke(name, args)`: the name is the path, the arguments are the
    /// JSON body.
    fn handle_binding_worker(
        &self,
        binding: &str,
        request: HttpRequest,
    ) -> OpFuture<'_, Result<HttpResponse, String>> {
        let is_command = binding == COMMAND_BINDING;
        let name = url::path(&request.url).trim_start_matches('/').to_string();

        Box::pin(async move {
            let unknown = || text(404, &format!("no command named '{name}'"));

            if !is_command {
                return Ok(unknown());
            }

            let body = request.body.collect().await?.unwrap_or_default();

            let Ok(args) = serde_json::from_slice(&body) else {
                return Ok(text(400, "the arguments are not JSON"));
            };

            let Some(call) = self.commands.call(&name, args) else {
                return Ok(unknown());
            };

            Ok(match call.await {
                Ok(value) => reply(200, "application/json", value.to_string()),
                Err(message) => text(500, &message),
            })
        })
    }
}
