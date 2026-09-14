use crate::multiplexer::types::Tab;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirectoryTrustRefusal {
    NoProvenance,
    NoCwd,
    CwdDiffersFromProvenance,
}

impl DirectoryTrustRefusal {
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::NoProvenance => "cwd was not created by CodeTerm for this tab",
            Self::NoCwd => "tab has no cwd",
            Self::CwdDiffersFromProvenance => "cwd differs from the directory CodeTerm created",
        }
    }
}

/// Provenance is host state written by the code that created the directory; a
/// spawn-supplied cwd never carries it, so it can never self-authorize trust.
pub(crate) fn directory_trust_provenance(tab: &Tab) -> Result<(), DirectoryTrustRefusal> {
    let provisioned = tab
        .provisioned_cwd
        .as_deref()
        .ok_or(DirectoryTrustRefusal::NoProvenance)?;
    let cwd = tab.cwd.as_deref().ok_or(DirectoryTrustRefusal::NoCwd)?;
    if crate::service::ServiceContext::paths_equal(cwd, provisioned) {
        Ok(())
    } else {
        Err(DirectoryTrustRefusal::CwdDiffersFromProvenance)
    }
}

#[cfg(test)]
mod tests {
    use super::{directory_trust_provenance, DirectoryTrustRefusal};
    use crate::multiplexer::state::MultiplexerState;

    fn state_with_tab(cwd: Option<&str>, provisioned: Option<&str>) -> (MultiplexerState, String) {
        let (mut state, _root) = MultiplexerState::new();
        let ws_id = state.list_workspaces()[0].id.clone();
        let tab_id = state.create_tab(&ws_id).expect("tab");
        let tab = state.get_tab_mut(&tab_id).expect("tab");
        tab.cwd = cwd.map(str::to_string);
        tab.provisioned_cwd = provisioned.map(str::to_string);
        (state, tab_id)
    }

    #[test]
    fn directory_trust_accepts_directory_codeterm_created() {
        let (state, tab_id) = state_with_tab(Some("/srv/lanes/a"), Some("/srv/lanes/a/"));
        assert_eq!(
            directory_trust_provenance(state.get_tab(&tab_id).unwrap()),
            Ok(())
        );
    }

    #[test]
    fn directory_trust_refuses_user_supplied_cwd_without_provenance() {
        let (state, tab_id) = state_with_tab(Some("/home/user/project"), None);
        assert_eq!(
            directory_trust_provenance(state.get_tab(&tab_id).unwrap()),
            Err(DirectoryTrustRefusal::NoProvenance)
        );
    }

    #[test]
    fn directory_trust_refuses_cwd_that_differs_from_provenance() {
        let (state, tab_id) = state_with_tab(Some("/home/user/project"), Some("/srv/lanes/a"));
        assert_eq!(
            directory_trust_provenance(state.get_tab(&tab_id).unwrap()),
            Err(DirectoryTrustRefusal::CwdDiffersFromProvenance)
        );
    }

    #[test]
    fn directory_trust_refuses_tab_without_cwd() {
        let (state, tab_id) = state_with_tab(None, Some("/srv/lanes/a"));
        assert_eq!(
            directory_trust_provenance(state.get_tab(&tab_id).unwrap()),
            Err(DirectoryTrustRefusal::NoCwd)
        );
    }

    #[test]
    fn directory_trust_provenance_survives_session_restore() {
        let (state, tab_id) = state_with_tab(Some("/srv/lanes/a"), Some("/srv/lanes/a"));
        let restored = MultiplexerState::from_session_snapshot(&state.to_session_snapshot());
        assert_eq!(
            directory_trust_provenance(restored.get_tab(&tab_id).expect("restored tab")),
            Ok(())
        );
    }

    #[test]
    fn spawn_payloads_cannot_supply_provenance() {
        let wire = serde_json::json!({
            "provider": "codex",
            "cwd": "/home/user/project",
            "provisioned_cwd": "/home/user/project",
        });
        let params: crate::service::SpawnAgentParams =
            serde_json::from_value(wire.clone()).expect("spawn params");
        assert_eq!(params.cwd.as_deref(), Some("/home/user/project"));
        assert!(params.provisioned_cwd.is_none());
        let spec: crate::service::spawn::AgentSpawnSpec =
            serde_json::from_value(wire).expect("agent spawn spec");
        assert!(spec.provisioned_cwd.is_none());
    }
}
