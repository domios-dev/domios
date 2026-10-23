use crate::service::remote_team::RemoteTeamMember;
use serde_json::{json, Value};

pub(super) fn member_sources(mut tabs: Vec<Value>, members: &[RemoteTeamMember]) -> Vec<Value> {
    for member in members {
        let source = tabs
            .iter()
            .position(|tab| tab["id"].as_str() == Some(member.tab_id.as_str()))
            .map(|index| tabs.remove(index));
        if member.finalized {
            continue;
        }
        let matches = source.as_ref().is_some_and(|tab| {
            tab["agent_session_id"].as_str() == Some(member.session_id.as_str())
                && tab["agent_type"].as_str() == Some(member.provider.as_str())
        });
        let mut tab = if matches {
            source.unwrap_or(Value::Null)
        } else {
            json!({
                "id": member.tab_id, "title": member.title, "agent_type": member.provider,
                "agent_session_id": member.session_id, "backing": {"kind":"pty"}, "pty_id": null,
                "chat_view": true, "role": "Worker", "role_profile":"worker", "depth":2,
                "remote_availability":"unavailable", "remote_membership_state":"unobserved_session",
            })
        };
        tab["remote_team_member"] = json!(member);
        tabs.push(tab);
    }
    tabs
}

pub(super) fn apply_member_placement(tab: &mut serde_json::Map<String, Value>) {
    let Some(member) = tab.get("remote_team_member").cloned() else {
        return;
    };
    tab.insert("team_id".into(), member["team_id"].clone());
    tab.insert("workspace_id".into(), member["workspace_id"].clone());
    tab.insert(
        "parent_tab_id".into(),
        member["orchestrator_tab_id"].clone(),
    );
}

#[cfg(test)]
mod tests;
