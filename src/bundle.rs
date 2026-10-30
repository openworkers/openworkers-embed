use crate::EmbedError;
use openworkers_core::{BindingInfo, Script};
use openworkers_transform::{CodeLanguage, parse_worker_code};

/// jsc dispatches to `addEventListener('fetch')` only, so a default export needs
/// wiring to it.
#[cfg(feature = "jsc")]
const DEFAULT_EXPORT_BRIDGE: &str = include_str!("jsc_bridge.js");

/// A worker ready to run.
#[derive(Clone)]
pub(crate) struct Bundle {
    script: Script,
}

impl Bundle {
    /// Lower `export default { fetch }` to a global the runtime dispatches to.
    pub fn compile(code: &str, bindings: Vec<BindingInfo>) -> Result<Self, EmbedError> {
        let code = parse_worker_code(code.as_bytes(), CodeLanguage::JavaScript)
            .map_err(EmbedError::Code)?;

        #[cfg(feature = "jsc")]
        let code = format!("{code}\n{DEFAULT_EXPORT_BRIDGE}");

        Ok(Self {
            script: Script::with_bindings(code, None, bindings),
        })
    }

    pub fn script(&self) -> Script {
        self.script.clone()
    }
}
