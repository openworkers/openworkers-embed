use crate::EmbedError;
use openworkers_core::{BindingInfo, Script};
use openworkers_transform::{CodeLanguage, parse_worker_code};

/// Defines `embed.invoke` for the worker code.
const SHIM: &str = include_str!("shim.js");

/// A worker ready to run.
#[derive(Clone)]
pub(crate) struct Bundle {
    script: Script,
}

impl Bundle {
    /// Lower `export default { fetch }` to a global the runtime dispatches to,
    /// and put the shim in front.
    pub fn compile(code: &str, bindings: Vec<BindingInfo>) -> Result<Self, EmbedError> {
        let code = parse_worker_code(code.as_bytes(), CodeLanguage::JavaScript)
            .map_err(EmbedError::Code)?;

        Ok(Self {
            script: Script::with_bindings(format!("{SHIM}{code}"), None, bindings),
        })
    }

    pub fn script(&self) -> Script {
        self.script.clone()
    }
}
