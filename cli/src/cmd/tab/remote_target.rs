#[derive(Debug, PartialEq, Eq)]
pub(super) enum RemoteTabTarget {
    Local,
    Remote(String, String),
    Invalid,
}

pub(crate) fn parse_remote_tab_id(value: &str) -> Option<(String, String)> {
    let (peer, tab) = value.strip_prefix("mesh:")?.split_once('/')?;
    if peer.is_empty()
        || tab.is_empty()
        || tab.contains('/')
        || value
            .chars()
            .any(|character| character.is_whitespace() || matches!(character, '?' | '&' | '#'))
    {
        return None;
    }
    Some((peer.to_string(), tab.to_string()))
}

pub(super) fn classify_remote_tab_id(value: &str) -> RemoteTabTarget {
    match parse_remote_tab_id(value) {
        Some((peer, tab)) => RemoteTabTarget::Remote(peer, tab),
        None if value.starts_with("mesh:") => RemoteTabTarget::Invalid,
        None => RemoteTabTarget::Local,
    }
}

pub(super) fn remote_tab_chat_path(remote_tab: &str, since: usize) -> String {
    format!(
        "/api/tabs/{}/chat?since={}",
        urlencoding::encode(remote_tab),
        urlencoding::encode(&since.to_string()),
    )
}

pub(super) fn remote_inspect_target(
    tabs: &[String],
) -> Result<Option<(String, Vec<String>)>, String> {
    let mut remote = Vec::new();
    let mut local = false;
    for value in tabs {
        match parse_remote_tab_id(value) {
            Some(identity) => remote.push(identity),
            None if value.starts_with("mesh:") => {
                return Err(format!("invalid remote tab id: {value}"));
            }
            None => local = true,
        }
    }
    if remote.is_empty() {
        return Ok(None);
    }
    if local {
        return Err("mixed local and remote inspection is unsupported".into());
    }
    let peer = remote[0].0.clone();
    if remote.iter().any(|(owner, _)| owner != &peer) {
        return Err("multi-peer remote inspection is unsupported".into());
    }
    Ok(Some((
        peer,
        remote.into_iter().map(|(_, tab)| tab).collect(),
    )))
}
