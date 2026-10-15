use super::{chatbackend_history, session_chat_snapshot, ChatMsg};

pub(super) fn read(
    svc: &crate::service::ServiceContext,
    tab_id: &str,
    since: usize,
) -> Result<(Vec<ChatMsg>, usize), String> {
    if let Some(snapshot) = super::runtime_stored::messages(svc, tab_id, since)? {
        return Ok(snapshot);
    }
    let (backing, provider, session, cwd) = {
        let mux = svc.mux.read();
        let tab = mux.get_tab(tab_id).ok_or("Tab not found")?;
        (
            tab.backing.clone(),
            tab.agent_type.clone(),
            tab.agent_session_id.clone(),
            tab.cwd.clone().unwrap_or_default(),
        )
    };
    let runtime = crate::service::agent_runtime::requested_for_tab(svc, tab_id);
    match backing {
        crate::multiplexer::types::TabBacking::Surface(_) => Ok((Vec::new(), 0)),
        crate::multiplexer::types::TabBacking::ChatBackend {
            plugin_id,
            session_id,
        } if !runtime => {
            let path = chatbackend_history::session_path(&plugin_id, &session_id);
            let messages = match chatbackend_history::try_read_messages(&path) {
                Ok(messages) => messages,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                Err(error) => return Err(format!("Stored chat transcript: {error}")),
            };
            let count = messages.len();
            Ok((messages.into_iter().skip(since).collect(), count))
        }
        backing => {
            let (provider, session) = match backing {
                crate::multiplexer::types::TabBacking::ChatBackend {
                    plugin_id,
                    session_id,
                } => (Some(plugin_id), Some(session_id)),
                _ => (provider, session),
            };
            match (provider, session.filter(|id| !id.is_empty())) {
                (Some(provider), Some(session)) => {
                    session_chat_snapshot(svc, &provider, &cwd, &session, since)
                }
                _ => Ok((Vec::new(), 0)),
            }
        }
    }
}
