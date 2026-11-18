use codeterm_storage::{DeskLayoutNode, DeskSnapshot};
use codeterm_types::config::provider_accounts::IMPLICIT_ACCOUNT;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct SavedAccount {
    pub id: String,
    pub namespace: Option<String>,
}

impl Default for SavedAccount {
    fn default() -> Self {
        Self {
            id: IMPLICIT_ACCOUNT.into(),
            namespace: None,
        }
    }
}

pub(super) fn from_snapshot(
    snapshot: &DeskSnapshot,
    provider: &str,
    session: &str,
) -> Result<SavedAccount, String> {
    from_layouts(
        snapshot
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.tabs.iter().map(|tab| &tab.layout)),
        provider,
        session,
    )
}

fn from_layouts<'a>(
    layouts: impl IntoIterator<Item = &'a DeskLayoutNode>,
    provider: &str,
    session: &str,
) -> Result<SavedAccount, String> {
    let mut nodes: Vec<_> = layouts.into_iter().collect();
    let mut selected = None;
    while let Some(node) = nodes.pop() {
        match node {
            DeskLayoutNode::Split { children, .. } => nodes.extend(children),
            DeskLayoutNode::Leaf { fields, .. } => {
                if fields.values["agent_type"].as_str() != Some(provider)
                    || fields.values["agent_session_id"].as_str() != Some(session)
                {
                    continue;
                }
                let launch = fields
                    .values
                    .pointer("/agent_identity/restorable_launch")
                    .filter(|value| !value.is_null())
                    .map(|value| {
                        serde_json::from_value::<
                            codeterm_pty::multiplexer::types::RestorableLaunchParameters,
                        >(value.clone())
                    })
                    .transpose()
                    .map_err(|error| format!("The saved account context is invalid: {error}"))?
                    .unwrap_or_default();
                let account = SavedAccount {
                    id: launch.account_id.unwrap_or_else(|| IMPLICIT_ACCOUNT.into()),
                    namespace: launch.account_namespace,
                };
                if selected.as_ref().is_some_and(|held| held != &account) {
                    return Err("The saved session has conflicting account contexts; resume its exact tab instead".into());
                }
                selected = Some(account);
            }
        }
    }
    Ok(selected.unwrap_or_default())
}

#[cfg(test)]
mod tests;
