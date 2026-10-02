use crate::multiplexer::types::Tab;
use crate::service::ServiceContext;

fn eligible_peer(target: &str, provider: &str, directory: &std::path::Path, peer: &Tab) -> bool {
    peer.id != target
        && peer.agent_type.as_deref() == Some(provider)
        && peer.launch_session_authenticated()
        && peer.pending_prompt.is_none()
        && peer
            .cwd
            .as_deref()
            .and_then(|cwd| std::fs::canonicalize(cwd).ok())
            .as_deref()
            == Some(directory)
}

impl ServiceContext {
    pub(crate) fn directory_trusted_by_live_peer(
        &self,
        tab_id: &str,
        provider: &str,
        cwd: &str,
    ) -> bool {
        let Ok(directory) = std::fs::canonicalize(cwd) else {
            return false;
        };
        let peers = {
            let mux = self.mux.read();
            mux.all_tabs().cloned().collect::<Vec<_>>()
        };
        peers
            .into_iter()
            .filter(|peer| eligible_peer(tab_id, provider, &directory, peer))
            .any(|peer| {
                let Some(pty_id) = peer.pty_id.as_deref() else {
                    return false;
                };
                if self.pty.lock().get_session(pty_id).is_none() {
                    return false;
                }
                self.observe_provider_process_tree(&peer.id, pty_id)
                    .is_some_and(|tree| {
                        tree.state() == super::super::process_tree::ProviderTreeState::Present
                    })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multiplexer::state::MultiplexerState;

    #[test]
    fn directory_trust_live_peer_requires_exact_directory_provider_and_verified_session() {
        let root = tempfile::tempdir().unwrap();
        let exact = std::fs::canonicalize(root.path()).unwrap();
        let (mut mux, peer_id) = MultiplexerState::new();
        let peer = mux.get_tab_mut(&peer_id).unwrap();
        peer.cwd = Some(root.path().to_string_lossy().into());
        peer.agent_type = Some("same-capability".into());
        peer.agent_session_id = Some("verified-session".into());
        peer.session_binding_nonce = Some("own-nonce".into());
        peer.reported_session_binding_nonce = Some("own-nonce".into());
        assert!(eligible_peer("target", "same-capability", &exact, peer));
        assert!(!eligible_peer(
            "target",
            "different-capability",
            &exact,
            peer
        ));
        assert!(!eligible_peer(&peer_id, "same-capability", &exact, peer));
        peer.reported_session_binding_nonce = None;
        assert!(!eligible_peer("target", "same-capability", &exact, peer));
        peer.reported_session_binding_nonce = Some("own-nonce".into());
        let child = root.path().join("child");
        std::fs::create_dir(&child).unwrap();
        assert!(!eligible_peer(
            "target",
            "same-capability",
            &std::fs::canonicalize(child).unwrap(),
            peer
        ));
        let missing = root.path().join("missing");
        peer.cwd = Some(missing.to_string_lossy().into());
        assert!(!eligible_peer("target", "same-capability", &exact, peer));
    }
}
