use super::{session_persist, spawn_cwd::validate_spawn_cwd, ServiceContext};
use serde::Serialize;

pub struct WorkspaceRelocationLock(pub std::sync::Arc<parking_lot::Mutex<()>>);

#[derive(Debug, Default, Serialize)]
pub struct WorkspaceRelocation {
    pub workspace_id: String,
    pub cwd: String,
    pub updated_tabs: Vec<String>,
    pub updated_plans: Vec<String>,
    pub warnings: Vec<String>,
}

pub(crate) fn rebase_path(path: &str, old_root: &str, new_root: &str) -> Option<String> {
    let windows = |value: &str| {
        let bytes = value.as_bytes();
        bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\')
    };
    let old_root = codeterm_paths::strip_verbatim_prefix(old_root);
    let old = old_root.replace('\\', "/");
    let path = codeterm_paths::strip_verbatim_prefix(path).replace('\\', "/");
    let is_windows = windows(&old);
    let old = if old == "/" {
        old.as_str()
    } else {
        old.trim_end_matches('/')
    };
    if old.is_empty() {
        return None;
    }
    let prefix = path.get(..old.len())?;
    let same = if windows(&path) && is_windows {
        prefix.eq_ignore_ascii_case(old)
    } else {
        prefix == old
    };
    let suffix = path.get(old.len()..)?;
    if !same
        || (!suffix.is_empty() && old != "/" && !suffix.starts_with('/'))
        || suffix.split('/').any(|part| matches!(part, "." | ".."))
    {
        return None;
    }
    let suffix = if old == "/" {
        format!("/{suffix}")
    } else {
        suffix.to_string()
    };
    let suffix = if new_root.contains('\\') {
        suffix.replace('/', "\\")
    } else {
        suffix
    };
    Some(format!(
        "{}{}",
        new_root.trim_end_matches(['/', '\\']),
        suffix
    ))
}

fn rebase_field(
    value: &mut Option<String>,
    old: &str,
    new: &str,
    label: &str,
    warnings: &mut Vec<String>,
) {
    let Some(path) = value.as_ref() else {
        return;
    };
    match rebase_path(path, old, new) {
        Some(path) => *value = Some(path),
        None => warnings.push(format!(
            "{label}: {path} is outside the old workspace folder; unchanged"
        )),
    }
}

fn relocate_tab_paths(
    tab: &mut crate::multiplexer::types::Tab,
    idle: bool,
    old: &str,
    new: &str,
    result: &mut WorkspaceRelocation,
) {
    let id = &tab.id;
    if !idle {
        result
            .warnings
            .push(format!("Tab {id} is active; its paths were left unchanged"));
        return;
    }
    let before = tab.cwd.clone();
    rebase_field(
        &mut tab.cwd,
        old,
        new,
        &format!("Tab {id}"),
        &mut result.warnings,
    );
    if let Some(restore) = tab.close_restore.as_mut() {
        rebase_field(
            &mut restore.session_cwd,
            old,
            new,
            &format!("Tab {id} stored session"),
            &mut result.warnings,
        );
    }
    if before != tab.cwd {
        result.updated_tabs.push(id.clone());
    }
}

impl ServiceContext {
    pub(crate) fn refresh_workspace_cwd_state(&self, workspace_id: &str) -> Result<bool, String> {
        let cwd = self
            .mux
            .read()
            .get_workspace(workspace_id)
            .ok_or_else(|| format!("Workspace not found: {workspace_id}"))?
            .cwd
            .clone();
        let missing = validate_spawn_cwd(cwd.as_deref()).is_err();
        let changed = {
            let mut mux = self.mux.write();
            let ws = mux
                .get_workspace_mut(workspace_id)
                .ok_or_else(|| format!("Workspace not found: {workspace_id}"))?;
            if ws.cwd != cwd {
                return Ok(ws.cwd_missing);
            }
            let changed = ws.cwd_missing != missing;
            ws.cwd_missing = missing;
            changed
        };
        if changed {
            self.emit_state_changed();
        }
        Ok(missing)
    }

    pub fn set_workspace_cwd(
        &self,
        workspace_id: &str,
        cwd: &str,
    ) -> Result<WorkspaceRelocation, String> {
        let _relocation = self
            .ext::<crate::service::workspace_cwd::WorkspaceRelocationLock>()
            .0
            .lock();
        if cwd.trim().is_empty() {
            return Err("Choose an existing absolute folder".into());
        }
        validate_spawn_cwd(Some(cwd))?;
        self.ensure_storage_available()?;
        let (workspace, tabs, teams) = {
            let mux = self.mux.read();
            let ws = mux
                .get_workspace(workspace_id)
                .ok_or_else(|| format!("Workspace not found: {workspace_id}"))?
                .clone();
            let tabs: Vec<_> = ws
                .tabs
                .iter()
                .filter_map(|id| mux.get_tab(id).cloned())
                .collect();
            let teams: Vec<_> = mux
                .snapshot()
                .teams
                .into_iter()
                .filter(|team| team.workspace_id == workspace_id)
                .map(|team| team.id)
                .collect();
            (ws, tabs, teams)
        };
        let mut result = WorkspaceRelocation {
            workspace_id: workspace_id.into(),
            cwd: cwd.into(),
            ..Default::default()
        };
        let old = workspace.cwd.as_deref().unwrap_or(cwd);
        let tab_ids: Vec<_> = tabs.iter().map(|tab| tab.id.clone()).collect();
        let (updated, warnings) = self
            .plan_store
            .relocate_workspace_projects(&tab_ids, &teams, |path| rebase_path(path, old, cwd))
            .map_err(|error| error.to_string())?;
        result.updated_plans = updated;
        result.warnings.extend(warnings);
        {
            let mut mux = self.mux.write();
            if mux.get_workspace(workspace_id).map(|ws| &ws.cwd) != Some(&workspace.cwd) {
                return Err(
                    "Workspace folder changed during relocation; choose the folder again".into(),
                );
            }
            for id in tab_ids {
                let Some(tab) = mux.get_tab_mut(&id) else {
                    continue;
                };
                let idle = !tab.agent_booting
                    && !crate::multiplexer::state::effective_tab_status(
                        tab,
                        self.resolved_tab_status(tab),
                    )
                    .has_active_turn();
                relocate_tab_paths(tab, idle, old, cwd, &mut result);
            }
            let ws = mux
                .get_workspace_mut(workspace_id)
                .ok_or_else(|| format!("Workspace not found: {workspace_id}"))?;
            ws.cwd = Some(cwd.into());
            ws.cwd_missing = false;
            ws.provisioned_cwd = None;
        }
        session_persist::persist_session_after_mutation(
            self,
            session_persist::SessionPersistUrgency::Prompt,
        )?;
        for plan_id in &result.updated_plans {
            self.emit_plan_changed(plan_id);
        }
        self.emit_state_changed();
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_rebase_preserves_suffix_and_refuses_neighbor_roots() {
        for (path, old, new, expected) in [
            (
                "/old/project/sub",
                "/old/project",
                "/new/project",
                Some("/new/project/sub"),
            ),
            (
                "/old/project",
                "/old/project/",
                "/new/project",
                Some("/new/project"),
            ),
            (
                "D:/Developer/codeterm/web",
                "d:\\Developer\\codeterm",
                "C:/Developer/codeterm",
                Some("C:/Developer/codeterm/web"),
            ),
            (
                "d:\\Developer\\codeterm\\web",
                "D:/Developer/codeterm",
                "C:/Developer/codeterm",
                Some("C:/Developer/codeterm/web"),
            ),
            ("/old/project-other", "/old/project", "/new", None),
            ("D:/other", "D:/Developer", "C:/Developer", None),
            ("/old/project/../other", "/old/project", "/new", None),
            ("/OLD/project", "/old/project", "/new", None),
            ("d:/sub", "D:/", "C:/new", Some("C:/new/sub")),
            ("/sub", "/", "/new", Some("/new/sub")),
        ] {
            assert_eq!(rebase_path(path, old, new).as_deref(), expected);
        }
    }
    #[test]
    fn idle_tab_relocation_preserves_session_team_and_launch_identity() {
        let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
        let ws = mux.list_workspaces()[0].id.clone();
        let id = mux.create_tab(&ws).unwrap();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.cwd = Some("D:/repo/sub".into());
        tab.agent_session_id = Some("session".into());
        tab.team_id = Some("team".into());
        tab.close_restore = Some(crate::multiplexer::types::CloseRestoreTabState {
            session_cwd: Some("D:/repo/sub".into()),
            exact_session: true,
            ..Default::default()
        });
        let mut result = WorkspaceRelocation::default();
        relocate_tab_paths(tab, false, "D:/repo", "C:/repo", &mut result);
        assert_eq!(tab.cwd.as_deref(), Some("D:/repo/sub"));
        assert_eq!(result.warnings.len(), 1);
        relocate_tab_paths(tab, true, "D:/repo", "C:/repo", &mut result);
        assert_eq!(tab.cwd.as_deref(), Some("C:/repo/sub"));
        assert_eq!(
            tab.close_restore.as_ref().unwrap().session_cwd.as_deref(),
            Some("C:/repo/sub")
        );
        assert!(tab.close_restore.as_ref().unwrap().exact_session);
        assert_eq!(tab.id, id);
        assert_eq!(tab.agent_session_id.as_deref(), Some("session"));
        assert_eq!(tab.team_id.as_deref(), Some("team"));
        assert_eq!(result.updated_tabs, vec![id]);
    }
}
