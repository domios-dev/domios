use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HolderProcess {
    pub pid: u32,
    pub created: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HolderTab {
    pub process: HolderProcess,
    pub tab_id: String,
    pub title: String,
    pub workspace: String,
    pub command: String,
    pub busy: bool,
}

pub const REFUSAL_PREFIX: &str = "Update blocked by busy tabs: ";

pub fn refusal(tabs: &[HolderTab]) -> Option<String> {
    let busy: Vec<_> = tabs
        .iter()
        .filter(|tab| tab.busy)
        .map(|tab| (&tab.tab_id, tab))
        .collect::<std::collections::BTreeMap<_, _>>()
        .into_values()
        .collect();
    if busy.is_empty() {
        return None;
    }
    let descriptions = busy
        .iter()
        .map(|tab| {
            format!(
                "{} in {}: {}. Close explicitly with `domios tab close --tab {}`",
                tab.title, tab.workspace, tab.command, tab.tab_id
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    let details = serde_json::to_string(&busy).unwrap_or_default();
    Some(format!(
        "{REFUSAL_PREFIX}{descriptions}. Then retry the update.\n[update-blocking-tabs]{details}"
    ))
}

#[cfg(test)]
mod tests;
