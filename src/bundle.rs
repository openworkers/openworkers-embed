use crate::EmbedError;
use openworkers_core::{BindingInfo, Script};
use openworkers_transform::{CodeLanguage, parse_worker_code};
use std::sync::Arc;

/// jsc dispatches to `addEventListener('fetch')` only, so a default export needs
/// wiring to it.
#[cfg(feature = "jsc")]
const DEFAULT_EXPORT_BRIDGE: &str = include_str!("jsc_bridge.js");

/// A worker ready to run. Each request carries one to its thread, and the
/// code is a few hundred kilobytes, so a clone shares it.
#[derive(Clone)]
pub(crate) struct Bundle {
    script: Arc<Script>,
}

impl Bundle {
    /// Lower `export default { fetch }` to a global the runtime dispatches to.
    pub fn compile(code: &str, bindings: Vec<BindingInfo>) -> Result<Self, EmbedError> {
        let code = parse_worker_code(code.as_bytes(), CodeLanguage::JavaScript)
            .map_err(EmbedError::Code)?;

        #[cfg(feature = "jsc")]
        let code = format!("{code}\n{DEFAULT_EXPORT_BRIDGE}");

        Ok(Self {
            script: Arc::new(Script::with_bindings(code, None, bindings)),
        })
    }

    /// A copy of the script, for a new context: the copy is the cost of one.
    pub fn script(&self) -> Script {
        Script::clone(&self.script)
    }
}
