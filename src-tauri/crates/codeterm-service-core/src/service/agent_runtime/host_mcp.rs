use crate::service::tab_credentials::TabCaller;
use codeterm_plugin::agent_runtime::RuntimeHostMcp;
use std::{collections::HashMap, path::PathBuf};

pub(super) fn minted_local(
    env: &HashMap<String, String>,
    expected: &TabCaller,
    identify: impl FnOnce(&str) -> Option<TabCaller>,
) -> bool {
    env.get("CODETERM_API_TOKEN")
        .filter(|token| !token.trim().is_empty())
        .and_then(|token| identify(token))
        .is_some_and(|caller| caller == *expected)
        && env.get("CODETERM_TAB_ID") == Some(&expected.tab_id)
        && env.get("CODETERM_SESSION_BINDING_NONCE") == Some(&expected.binding_nonce)
        && env
            .get("CODETERM_API_URL")
            .is_some_and(|url| !url.trim().is_empty())
}

pub(super) fn descriptor(
    declared: bool,
    minted_local_credential: bool,
    tab: &str,
    nonce: &str,
    executable: Option<PathBuf>,
    environment: &HashMap<String, String>,
) -> Option<RuntimeHostMcp> {
    if !declared || !minted_local_credential || tab.is_empty() || nonce.is_empty() {
        return None;
    }
    let command = executable?.to_str()?.to_owned();
    let mut env: std::collections::BTreeMap<String, String> = [
        "CODETERM_API_TOKEN",
        "CODETERM_API_URL",
        "CODETERM_TAB_ID",
        "CODETERM_SESSION_BINDING_NONCE",
    ]
    .into_iter()
    .map(|key| Some((key.to_owned(), environment.get(key)?.clone())))
    .collect::<Option<_>>()?;
    // The task identity selects the tab's own ACK; agents may not forward their env.
    env.extend(
        ["CODETERM_PLAN_ID", "CODETERM_TASK_ID"]
            .into_iter()
            .filter_map(|key| Some((key.to_owned(), environment.get(key)?.clone())))
            .filter(|(_, value)| !value.trim().is_empty()),
    );
    Some(RuntimeHostMcp {
        env,
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
