use codeterm_plugin::agent_runtime::RuntimeHostMcp;
use std::path::PathBuf;

pub(super) fn descriptor(
    declared: bool,
    minted_local_credential: bool,
    tab: &str,
    nonce: &str,
    executable: Option<PathBuf>,
) -> Option<RuntimeHostMcp> {
    if !declared || !minted_local_credential || tab.is_empty() || nonce.is_empty() {
        return None;
    }
    let command = executable?.to_str()?.to_owned();
    Some(RuntimeHostMcp {
        name: "domios".into(),
        command,
        args: vec![
            "mcp".into(),
            "serve".into(),
            "--tab".into(),
            tab.into(),
            "--binding-nonce".into(),
            nonce.into(),
        ],
    })
}

#[cfg(test)]
mod tests;
