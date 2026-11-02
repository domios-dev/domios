use super::*;

pub(super) type FixtureOperation =
    Box<dyn FnOnce(&mut PersistenceBackend) -> Result<FixtureCommitted, FixtureFailure> + Send>;

#[derive(Debug)]
pub(super) struct FixtureCommitted {
    pub(super) tabs: usize,
    pub(super) mux: serde_json::Value,
}

#[derive(Debug)]
pub(super) struct FixtureFailure {
    pub(super) reason: String,
    pub(super) terminal: bool,
}

impl From<String> for FixtureFailure {
    fn from(reason: String) -> Self {
        Self {
            reason,
            terminal: false,
        }
    }
}

pub fn import_fixture(
    svc: &crate::service::ServiceContext,
    snapshot: crate::session::SessionSnapshot,
    plans: Vec<codeterm_types::plan::Plan>,
    prepare: impl FnOnce() -> Result<(), String> + Send + 'static,
) -> Result<usize, String> {
    let worker = ensure_worker(svc)?;
    let service = svc.weak_self().upgrade().ok_or("Service unavailable")?;
    let (response, completion) = mpsc::channel();
    worker.configure(WorkerMessage::Fixture {
        operation: Box::new(move |backend| {
            commit_fixture(&service, backend, snapshot, &plans, prepare, || {})
        }),
        response,
    })?;
    completion.recv_timeout(PERSISTENCE_DEADLINE).map_err(|error| {
        format!("Fixture stage=completion desk=uncertain plans=uncertain mux=uncertain; do not replay: {error}")
    })?
}

pub fn validate_fixture_snapshot(
    snapshot: &crate::session::SessionSnapshot,
) -> Result<codeterm_storage::DeskSnapshot, String> {
    use crate::session::store::SplitNodeSnapshot;
    if snapshot
        .workspaces
        .iter()
        .all(|workspace| workspace.tabs.is_empty())
    {
        return Err("Fixture import requires at least one leaf tab".into());
    }
    let workspace_ids: HashSet<_> = snapshot
        .workspaces
        .iter()
        .map(|ws| ws.id.as_str())
        .collect();
    if snapshot
        .active_workspace_id
        .as_deref()
        .is_some_and(|id| !workspace_ids.contains(id))
        || snapshot
            .teams
            .iter()
            .any(|team| !workspace_ids.contains(team.workspace_id.as_str()))
    {
        return Err("Fixture workspace reference is missing".into());
    }
    let mut sessions = HashSet::new();
    for workspace in &snapshot.workspaces {
        if workspace
            .active_tab_id
            .as_deref()
            .is_some_and(|id| !workspace.tabs.iter().any(|tab| tab.id == id))
        {
            return Err(format!("Fixture active tab is missing: {}", workspace.id));
        }
        for tab in &workspace.tabs {
            let SplitNodeSnapshot::Leaf {
                tab_id,
                chatbackend_plugin_id,
                chatbackend_session_id,
                ..
            } = &tab.split_tree
            else {
                return Err(
                    "Fixture split layouts cannot be published without dropping leaves".into(),
                );
            };
            if tab.id != *tab_id {
                return Err(format!("Fixture group/leaf identity differs: {}", tab.id));
            }
            let mut leaf_sessions = HashSet::new();
            if let Some((_, session)) = tab.split_tree.agent_session() {
                leaf_sessions.insert(session);
            }
            if chatbackend_plugin_id.is_some() != chatbackend_session_id.is_some() {
                return Err(format!(
                    "Fixture chat backend identity is incomplete: {tab_id}"
                ));
            }
            if let Some(session) = chatbackend_session_id.as_deref() {
                leaf_sessions.insert(session);
            }
            for session in leaf_sessions {
                if !sessions.insert(session) {
                    return Err(format!("Fixture session identity collision: {session}"));
                }
            }
        }
    }
    let projected =
        crate::session::store::project_snapshot(snapshot, desk_agent_session_identities(snapshot))?;
    projected.validate().map_err(|error| error.to_string())?;
    Ok(projected)
}

fn session_entities(keys: &mut HashSet<String>, provider: &str, session: &str) {
    keys.insert(format!("session/{provider}/{session}"));
    keys.insert(format!("session-id/{session}"));
}

fn fixture_entities(snapshot: &codeterm_storage::DeskSnapshot) -> HashSet<String> {
    fn layout(node: &codeterm_storage::DeskLayoutNode, keys: &mut HashSet<String>) {
        match node {
            codeterm_storage::DeskLayoutNode::Leaf { tab_id, fields, .. } => {
                keys.insert(format!("tab/{tab_id}"));
                for (provider, session) in [
                    ("agent_type", "agent_session_id"),
                    ("chatbackend_plugin_id", "chatbackend_session_id"),
                ] {
                    if let (Some(provider), Some(session)) = (
                        fields.values[provider].as_str(),
                        fields.values[session].as_str(),
                    ) {
                        session_entities(keys, provider, session);
                    }
                }
            }
            codeterm_storage::DeskLayoutNode::Split { children, .. } => {
                for child in children {
                    layout(child, keys);
                }
            }
        }
    }
    let mut keys = HashSet::new();
    for ws in &snapshot.workspaces {
        keys.insert(format!("workspace/{}", ws.record.id.as_str()));
        for tab in &ws.tabs {
            keys.insert(format!("tab/{}", tab.id));
            layout(&tab.layout, &mut keys);
        }
    }
    keys.extend(
        snapshot
            .teams
            .iter()
            .map(|team| format!("team/{}", team.id)),
    );
    for id in &snapshot.agent_session_identities {
        session_entities(&mut keys, &id.provider, &id.session_id);
    }
    keys
}

pub(super) fn fixture_mux_value(
    mux: &crate::multiplexer::state::MultiplexerState,
) -> Result<serde_json::Value, String> {
    let snapshot = mux.snapshot();
    let mut value = serde_json::to_value(&snapshot).map_err(|error| error.to_string())?;
    value["tabs"] = serde_json::to_value(&snapshot.tabs).map_err(|error| error.to_string())?;
    let mut persistent = mux.to_session_snapshot();
    persistent.timestamp = 0;
    value["persistent"] = serde_json::to_value(persistent).map_err(|error| error.to_string())?;
    value
        .as_object_mut()
        .ok_or("Invalid mux snapshot")?
        .remove("generation");
    value
        .as_object_mut()
        .ok_or("Invalid mux snapshot")?
        .remove("recently_closed");
    value
        .as_object_mut()
        .ok_or("Invalid mux snapshot")?
        .remove("closed_structures");
    Ok(value)
}

fn stage_fixture(
    mux: &crate::multiplexer::state::MultiplexerState,
    merged: &crate::session::SessionSnapshot,
) -> Result<crate::multiplexer::state::TeamCloseTransaction, String> {
    let mut staged = mux.begin_team_close_transaction();
    let mut without_paths = merged.clone();
    for workspace in &mut without_paths.workspaces {
        workspace.cwd = None;
    }
    staged.state_mut().merge_session_snapshot(&without_paths);
    for workspace in &merged.workspaces {
        if mux.get_workspace(&workspace.id).is_none() {
            let target = staged
                .state_mut()
                .get_workspace_mut(&workspace.id)
                .ok_or("Fixture workspace publication missing")?;
            target.cwd = workspace.cwd.clone();
            target.provisioned_cwd = workspace.provisioned_cwd.clone();
            target.protected = workspace.protected;
        }
    }
    for team in &merged.teams {
        if mux.get_team(&team.id).is_none() {
            staged.state_mut().upsert_team(team.clone());
        }
    }
    if mux.snapshot().active_workspace_id.is_none() {
        if let Some(id) = &merged.active_workspace_id {
            staged.state_mut().set_active_workspace(id)?;
        }
    }
    Ok(staged)
}

pub(super) fn commit_fixture(
    svc: &crate::service::ServiceContext,
    backend: &mut PersistenceBackend,
    snapshot: crate::session::SessionSnapshot,
    plans: &[codeterm_types::plan::Plan],
    before_submission: impl FnOnce() -> Result<(), String>,
    after_submission: impl FnOnce(),
) -> Result<FixtureCommitted, FixtureFailure> {
    let PersistenceBackend::Storage { adapter, revision } = backend else {
        return Err("Fixture import requires initialized desk persistence"
            .to_string()
            .into());
    };
    let incoming = validate_fixture_snapshot(&snapshot)?;
    let state = adapter.state()?;
    let mut current_keys = fixture_entities(&state.snapshot);
    let before = {
        let mux = svc.mux.read();
        let value = fixture_mux_value(&mux)?;
        if mux.snapshot().tabs.values().any(|tab| tab.pty_id.is_some()) {
            return Err("Fixture import refuses a DEV desk owning PTYs"
                .to_string()
                .into());
        }
        for tab in mux.snapshot().tabs.values() {
            current_keys.insert(format!("tab/{}", tab.id));
            if let (Some(provider), Some(session)) = (&tab.agent_type, &tab.agent_session_id) {
                session_entities(&mut current_keys, provider, session);
            }
            if let crate::multiplexer::types::TabBacking::ChatBackend {
                plugin_id,
                session_id,
            } = &tab.backing
            {
                session_entities(&mut current_keys, plugin_id, session_id);
            }
        }
        current_keys.extend(
            mux.list_workspaces()
                .iter()
                .map(|ws| format!("workspace/{}", ws.id)),
        );
        current_keys.extend(
            mux.list_teams()
                .iter()
                .map(|team| format!("team/{}", team.id)),
        );
        value
    };
    let incoming_keys = fixture_entities(&incoming);
    if let Some(key) = incoming_keys.intersection(&current_keys).min() {
        return Err(format!("Fixture identity collision: {key}").into());
    }
    let all_keys: HashSet<_> = current_keys.union(&incoming_keys).cloned().collect();
    for workspace in &snapshot.workspaces {
        for tab in &workspace.tabs {
            let crate::session::store::SplitNodeSnapshot::Leaf {
                parent_tab_id,
                team_id,
                agent_identity,
                ..
            } = &tab.split_tree
            else {
                return Err("Fixture publication requires leaf tabs".to_string().into());
            };
            for (kind, id) in [("tab", parent_tab_id), ("team", team_id)] {
                if let Some(id) = id {
                    if !all_keys.contains(&format!("{kind}/{id}")) {
                        return Err(format!("Fixture reference missing: {kind}/{id}").into());
                    }
                }
            }
            if let Some(team) = team_id {
                if !snapshot
                    .teams
                    .iter()
                    .any(|row| &row.id == team && row.workspace_id == workspace.id)
                {
                    return Err(format!("Fixture Team workspace reference differs: {team}").into());
                }
            }
            if let Some(binding) = agent_identity
                .as_ref()
                .and_then(|identity| identity.task_binding.as_ref())
            {
                if let Some(id) = &binding.plan_id {
                    let plan = plans
                        .iter()
                        .find(|plan| &plan.id == id)
                        .ok_or_else(|| format!("Fixture plan reference missing: {id}"))?;
                    if let Some(task_id) = &binding.task_id {
                        let task = plan
                            .tasks
                            .iter()
                            .find(|task| &task.id == task_id)
                            .ok_or_else(|| {
                                format!("Fixture task reference missing: {id}/{task_id}")
                            })?;
                        if binding.attempt_id.as_ref().is_some_and(|id| {
                            !task
                                .execution_attempts
                                .iter()
                                .any(|attempt| &attempt.id == id)
                        }) {
                            return Err(format!(
                                "Fixture attempt reference missing: {id}/{task_id}"
                            )
                            .into());
                        }
                    }
                }
            }
        }
    }
    for plan in plans {
        if plan
            .team_id
            .as_ref()
            .is_some_and(|id| !all_keys.contains(&format!("team/{id}")))
        {
            return Err(format!("Fixture plan Team reference missing: {}", plan.id).into());
        }
    }
    let mut merged = adapter.snapshot()?;
    merged.workspaces.extend(snapshot.workspaces.clone());
    merged.teams.extend(snapshot.teams.clone());
    if merged.active_workspace_id.is_none() {
        merged.active_workspace_id = snapshot.active_workspace_id.clone();
    }
    let mut normalized = crate::session::store::project_snapshot(
        &merged,
        state.snapshot.agent_session_identities.clone(),
    )?;
    normalized
        .agent_session_identities
        .extend(incoming.agent_session_identities);
    normalized.validate().map_err(|error| error.to_string())?;
    let count = snapshot.workspaces.iter().map(|ws| ws.tabs.len()).sum();
    stage_fixture(&svc.mux.read(), &merged)?;
    let mut submitted = false;
    let mut desk_committed = false;
    let mut plans_committed = false;
    let result = svc.plan_store.import_fixture_plans(
        plans,
        || {
            before_submission()?;
            let completion = {
                let mux = svc.mux.read();
                if fixture_mux_value(&mux)? != before
                    || mux.snapshot().tabs.values().any(|tab| tab.pty_id.is_some())
                {
                    return Err("Fixture mux changed before submission".into());
                }
                let completion = adapter.delta_snapshot(
                    &merged,
                    normalized.agent_session_identities,
                    state.revision,
                    SaveIntent::PreserveDesk,
                )?;
                submitted = true;
                completion
            };
            after_submission();
            match completion.wait(PERSISTENCE_DEADLINE) {
                Ok(committed) => {
                    *revision = committed.receipt.revision;
                    desk_committed = true;
                    Ok(())
                }
                Err(error @ codeterm_storage::StorageError::StaleRevision { .. }) => {
                    submitted = false;
                    Err(format!("Fixture competing desk revision: {error}"))
                }
                Err(error) => Err(error.to_string()),
            }
        },
        |()| {
            plans_committed = true;
            let mut mux = svc.mux.write();
            if fixture_mux_value(&mux)? != before {
                return Err("Fixture mux publication refused: live state changed".into());
            }
            let mut staged = stage_fixture(&mux, &merged)?;
            let published = fixture_mux_value(staged.state_mut())?;
            staged.commit(&mut mux, &[]);
            Ok(FixtureCommitted {
                tabs: count,
                mux: published,
            })
        },
    );
    result.map_err(|reason| FixtureFailure {
        reason: format!(
            "Fixture stage={} desk={} plans={} mux=not_published; {}{reason}",
            if !submitted {
                "admission"
            } else if !desk_committed {
                "desk"
            } else if !plans_committed {
                "plans"
            } else {
                "publication"
            },
            if desk_committed {
                "committed"
            } else if submitted {
                "uncertain"
            } else {
                "not_committed"
            },
            if plans_committed {
                "committed"
            } else if desk_committed {
                "uncertain"
            } else {
                "not_started"
            },
            if submitted { "do not replay: " } else { "" }
        ),
        terminal: submitted,
    })
}
