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
    pub pty_id: String,
    pub pty_generation: u64,
    pub input_revision: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReserveHolders {
    pub tabs: Vec<HolderTab>,
    pub initiating_tab: Option<String>,
}

pub const REFUSAL_PREFIX: &str = "Update blocked by busy tabs: ";

#[derive(Serialize)]
pub struct BusyTab {
    pub tab_id: String,
    pub title: String,
    pub workspace: String,
    pub command: String,
}

pub fn refusal(tabs: impl IntoIterator<Item = BusyTab>) -> Option<String> {
    let busy: Vec<_> = tabs
        .into_iter()
        .map(|tab| (tab.tab_id.clone(), tab))
        .collect::<std::collections::BTreeMap<_, _>>()
        .into_values()
        .collect();
    if busy.is_empty() {
        return None;
    }
    let descriptions = busy
        .iter()
        .map(|tab| format!("{} in {}: {}", tab.title, tab.workspace, tab.command))
        .collect::<Vec<_>>()
        .join("; ");
    let details = serde_json::to_string(&busy).unwrap_or_default();
    Some(format!(
        "{REFUSAL_PREFIX}{descriptions}. Wait for these tabs to finish, then retry the update.\n[update-blocking-tabs]{details}"
    ))
}

#[cfg(test)]
mod tests;
