use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::Tab;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LaunchOwnership {
    Main,
    FreshSession,
}

#[derive(Default)]
pub(super) struct FreshLaunchSelection {
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub preset_args: Vec<String>,
}

pub(super) fn default_selection(
    config: &crate::config::schema::GeneralAgentConfig,
) -> FreshLaunchSelection {
    FreshLaunchSelection {
        model: config.model.clone(),
        reasoning_effort: config.reasoning_effort.clone(),
        preset_args: Vec::new(),
    }
}

pub(super) struct TabLaunch<'a> {
    pub tab_id: String,
    pub session_id: Option<String>,
    pub provider: &'a str,
    pub permission_bypass: bool,
    pub ownership: LaunchOwnership,
    pub selection: FreshLaunchSelection,
}

impl LaunchOwnership {
    pub(super) fn select_session(self, session: Option<String>) -> Option<String> {
        session.filter(|_| self == Self::Main)
    }

    pub(super) fn bootstrap(self) -> bool {
        self == Self::Main
    }

    pub(super) fn persist_session_id(self, provider: &str, session: &str) {
        self.with_main_state(|| super::state::persist_session_id(provider, session));
    }

    fn with_main_state(self, write: impl FnOnce()) {
        if self == Self::Main {
            write();
        }
    }
}

pub(crate) fn owns_main(mux: &MultiplexerState, tab: &Tab) -> bool {
    super::is_general_agent(tab)
        && mux.resolve_tab_id(super::GENERAL_ALIAS).as_deref() == Some(tab.id.as_str())
}

pub(super) fn sanctioned_session(tab: &Tab) -> bool {
    let home = super::general_dir();
    tab.provisioned_cwd.as_deref().map(std::path::Path::new) == Some(home.as_path())
        && tab.team_id.is_none()
        && tab.parent_tab_id.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_main_selection_reads_saved_general_defaults() {
        let config = crate::config::schema::GeneralAgentConfig {
            model: Some("model-token".into()),
            reasoning_effort: Some("effort-token".into()),
            ..Default::default()
        };
        let restored = serde_json::from_value(serde_json::to_value(config).unwrap()).unwrap();
        let selection = default_selection(&restored);
        assert_eq!(selection.model.as_deref(), Some("model-token"));
        assert_eq!(selection.reasoning_effort.as_deref(), Some("effort-token"));
    }
    #[test]
    fn fresh_session_never_reads_bootstrap_or_writes_main_state() {
        let mut writes = 0;
        LaunchOwnership::FreshSession.with_main_state(|| writes += 1);
        assert_eq!(writes, 0);
        assert!(!LaunchOwnership::FreshSession.bootstrap());
        LaunchOwnership::Main.with_main_state(|| writes += 1);
        assert_eq!(writes, 1);
        assert!(LaunchOwnership::Main.bootstrap());
    }

    #[test]
    fn fresh_selection_refuses_a_main_snapshot_session() {
        assert_eq!(
            LaunchOwnership::FreshSession.select_session(Some("main-session".into())),
            None
        );
        assert_eq!(
            LaunchOwnership::Main.select_session(Some("main-session".into())),
            Some("main-session".into())
        );
    }

    #[test]
    fn main_alias_alone_owns_automatic_recovery() {
        let mut mux = MultiplexerState::empty();
        super::super::ensure_general_workspace(&mut mux, false);
        let main = mux.resolve_tab_id(super::super::GENERAL_ALIAS).unwrap();
        let workspace = mux.find_workspace_for_tab(&main).unwrap();
        let sibling = mux.create_tab(&workspace).unwrap();
        super::super::identity::apply_general_agent_kind(mux.get_tab_mut(&sibling).unwrap());
        assert!(owns_main(&mux, mux.get_tab(&main).unwrap()));
        assert!(!owns_main(&mux, mux.get_tab(&sibling).unwrap()));
    }
}
