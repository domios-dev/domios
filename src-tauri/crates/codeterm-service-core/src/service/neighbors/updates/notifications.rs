pub(super) use codeterm_neighbors::registry::Acceptance;
use codeterm_neighbors::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Default, Deserialize, Serialize)]
pub(super) struct Seen {
    incoming: Vec<String>,
    accepted: Vec<String>,
}

pub(super) struct Notice {
    pub event_id: String,
    pub title: String,
}

fn name(profile: &Value) -> Result<&str> {
    profile["displayName"]
        .as_str()
        .filter(|name| !name.is_empty())
        .or_else(|| profile["uid"].as_str())
        .filter(|name| !name.is_empty() && name.len() <= 512)
        .ok_or(Error::Transport)
}

fn rows<'a>(list: &'a Value, key: &str) -> Result<&'a [Value]> {
    list[key]
        .as_array()
        .filter(|rows| rows.len() <= 200)
        .map(Vec::as_slice)
        .ok_or(Error::Transport)
}

fn remember(ids: &mut Vec<String>, id: String) {
    ids.push(id);
    let excess = ids.len().saturating_sub(200);
    ids.drain(..excess);
}

pub(super) fn changes(
    seen: &mut Seen,
    list: &Value,
    accepted: &[Acceptance],
) -> Result<Vec<Notice>> {
    if seen.incoming.len() > 200 || seen.accepted.len() > 200 {
        return Err(Error::CursorStore);
    }
    let incoming = rows(list, "incoming")?;
    let friends = rows(list, "friends")?;
    if accepted.len() > 200 {
        return Err(Error::Transport);
    }
    let mut notices = vec![];
    for profile in incoming {
        let id = profile["request_id"].as_str().ok_or(Error::Transport)?;
        if seen.incoming.iter().any(|previous| previous == id) {
            continue;
        }
        notices.push(Notice {
            event_id: format!("request:{id}"),
            title: format!("{} wants to be your Neighbor", name(profile)?),
        });
        remember(&mut seen.incoming, id.into());
    }
    for acceptance in accepted {
        if seen.accepted.contains(&acceptance.event_id) {
            continue;
        }
        let Some(profile) = friends
            .iter()
            .find(|friend| friend["friendship_id"] == acceptance.friendship_id)
        else {
            continue;
        };
        notices.push(Notice {
            event_id: format!("accepted:{}", acceptance.event_id),
            title: format!("{} accepted your request", name(profile)?),
        });
        remember(&mut seen.accepted, acceptance.event_id.clone());
    }
    Ok(notices)
}

#[cfg(test)]
mod tests;
