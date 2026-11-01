use crate::EmbedError;
use openworkers_core::{BindingInfo, Script};
use openworkers_transform::{CodeLanguage, parse_worker_code};
use std::collections::HashMap;

/// jsc and boa dispatch to `addEventListener('fetch')` only, so a default
/// export needs wiring to it.
#[cfg(any(feature = "jsc", feature = "boa"))]
const DEFAULT_EXPORT_BRIDGE: &str = include_str!("default_export_bridge.js");

/// A worker ready to run.
#[derive(Clone)]
pub(crate) struct Bundle {
    script: Script,
}

impl Bundle {
    /// Lower `export default { fetch }` to a global the runtime dispatches to.
    pub fn compile(
        code: &str,
        vars: HashMap<String, String>,
        bindings: Vec<BindingInfo>,
    ) -> Result<Self, EmbedError> {
        let code = parse_worker_code(code.as_bytes(), CodeLanguage::JavaScript)
            .map_err(EmbedError::Code)?;

        #[cfg(any(feature = "jsc", feature = "boa"))]
        let code = format!("{code}\n{DEFAULT_EXPORT_BRIDGE}");

        Ok(Self {
            script: Script::with_bindings(code, Some(vars), bindings),
        })
    }

    pub fn script(&self) -> Script {
        self.script.clone()
    }
}
