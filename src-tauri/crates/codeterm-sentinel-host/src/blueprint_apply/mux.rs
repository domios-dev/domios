use std::collections::{BTreeMap, BTreeSet};

use codeterm_blueprint_types::blueprint_pipeline::ResultTarget;
use codeterm_service_core::multiplexer::{state::MultiplexerState, types::Tab};
use codeterm_types::team::Team;

use crate::{graph::MuxView, machine::Route, service_adapter::SentinelCaller, store::Sentinel};

pub(crate) trait ScopeView: MuxView {
    fn resolve_tab_id(&self, id: &str) -> Option<String>;
    fn all_tabs(&self) -> Box<dyn Iterator<Item = &Tab> + '_>;
}

impl ScopeView for MultiplexerState {
    fn resolve_tab_id(&self, id: &str) -> Option<String> {
        self.resolve_tab_id(id)
    }

    fn all_tabs(&self) -> Box<dyn Iterator<Item = &Tab> + '_> {
        Box::new(self.all_tabs())
    }
}

pub(super) struct Snapshot {
    workspace: Option<String>,
    tabs: BTreeMap<String, (Tab, Option<String>)>,
    team: Option<(Team, Option<String>)>,
    aliases: BTreeMap<String, String>,
}

pub(super) struct References {
    tab_ids: BTreeSet<String>,
    result: Option<ResultTarget>,
}

impl References {
    pub(super) fn collect(caller: &SentinelCaller, candidate: &Sentinel) -> Self {
        let mut tab_ids = BTreeSet::from([caller.tab.clone()]);
        if let Some(crate::source::TabBinding::Tab(id)) = candidate.observe() {
            tab_ids.insert(id.clone());
        }
        for handler in candidate
            .routes
            .values()
            .flatten()
            .chain(&candidate.on_other)
        {
            if let Route::Send(id) = &handler.action {
                tab_ids.insert(id.clone());
            }
        }
        Self {
            tab_ids,
            result: candidate.pipeline().map(|spec| spec.result.to.clone()),
        }
    }
}

impl Snapshot {
    pub(super) fn capture(
        mux: &MultiplexerState,
        caller: &SentinelCaller,
        references: References,
    ) -> Self {
        let References {
            mut tab_ids,
            result,
        } = references;
        let mut aliases = BTreeMap::new();
        if let Some(target) = result {
            match &target {
                ResultTarget::Tab(id) => {
                    if let Some(resolved) = mux.resolve_tab_id(id) {
                        tab_ids.insert(resolved.clone());
                        aliases.insert(id.clone(), resolved);
                    }
                }
                ResultTarget::Team => {
                    tab_ids.extend(
                        mux.all_tabs()
                            .filter(|tab| tab.team_id == caller.team && tab.anchors_team())
                            .map(|tab| tab.id.clone()),
                    );
                }
                ResultTarget::Owner | ResultTarget::Notify => {}
            }
        }
        Self {
            workspace: mux
                .get_workspace(&caller.workspace)
                .map(|workspace| workspace.id.clone()),
            tabs: tab_ids
                .into_iter()
                .filter_map(|id| {
                    mux.get_tab(&id).map(|tab| {
                        let workspace = mux.find_workspace_for_tab(&id);
                        (id, (tab.clone(), workspace))
                    })
                })
                .collect(),
            team: caller.team.as_deref().and_then(|id| {
                mux.get_team(id)
                    .map(|team| (team.clone(), mux.team_workspace(id)))
            }),
            aliases,
        }
    }
}

impl MuxView for Snapshot {
    fn has_workspace(&self, workspace: &str) -> bool {
        self.workspace.as_deref() == Some(workspace)
    }

    fn get_tab(&self, id: &str) -> Option<&Tab> {
        self.tabs.get(id).map(|(tab, _)| tab)
    }

    fn find_workspace_for_tab(&self, id: &str) -> Option<String> {
        self.tabs
            .get(id)
            .and_then(|(_, workspace)| workspace.clone())
    }

    fn get_team(&self, id: &str) -> Option<&Team> {
        self.team
            .as_ref()
            .map(|(team, _)| team)
            .filter(|team| team.id == id)
    }

    fn team_workspace(&self, id: &str) -> Option<String> {
        self.team
            .as_ref()
            .filter(|(team, _)| team.id == id)
            .and_then(|(_, workspace)| workspace.clone())
    }
}

impl ScopeView for Snapshot {
    fn resolve_tab_id(&self, id: &str) -> Option<String> {
        self.aliases.get(id).cloned()
    }

    fn all_tabs(&self) -> Box<dyn Iterator<Item = &Tab> + '_> {
        Box::new(self.tabs.values().map(|(tab, _)| tab))
    }
}
