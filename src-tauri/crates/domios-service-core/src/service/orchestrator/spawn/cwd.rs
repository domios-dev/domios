use crate::service::ServiceContext;
impl ServiceContext {
    pub(crate) fn normalize_resolved_agent_cwd(path: &str) -> String {
        codeterm_paths::strip_verbatim_prefix(path)
    }

    /// Resolve agent cwd against workspace cwd:
    /// - If no cwd specified → inherit from workspace
    /// - If cwd differs from workspace → return warning
    /// - If nothing is specified → pin the daemon's own cwd, the dir the PTY
    ///   would inherit anyway. Leaving it `None` desyncs the tab record from
    ///   where the agent actually runs, so provider session-file resolution
    ///   (`~/.claude/projects/<encoded-cwd>/<sid>.jsonl`) looks in the wrong
    ///   dir and the chat pipeline reads empty.
    pub(crate) fn resolve_agent_cwd(
        agent_cwd: &Option<String>,
        workspace_cwd: &Option<String>,
    ) -> (Option<String>, Option<String>) {
        let agent_cwd = agent_cwd.as_deref().map(Self::normalize_resolved_agent_cwd);
        let workspace_cwd = workspace_cwd
            .as_deref()
            .map(Self::normalize_resolved_agent_cwd);
        match (agent_cwd.as_deref(), workspace_cwd.as_deref()) {
            (None, Some(ws_cwd)) => {
                log::info!("[spawn] agent cwd inherited from workspace: {}", ws_cwd);
                (Some(ws_cwd.to_string()), None)
            }
            (Some(agent_dir), Some(ws_dir)) if !Self::paths_equal(agent_dir, ws_dir) => {
                let warning = format!(
                    "Agent cwd '{}' differs from workspace cwd '{}'. \
                     The agent will run in '{}', but the workspace is at '{}'.",
                    agent_dir, ws_dir, agent_dir, ws_dir
                );
                log::warn!("[spawn] {}", warning);
                (Some(agent_dir.to_string()), Some(warning))
            }
            (Some(dir), _) => (Some(dir.to_string()), None),
            (None, None) => (
                std::env::current_dir()
                    .ok()
                    .and_then(|p| p.to_str().map(Self::normalize_resolved_agent_cwd)),
                None,
            ),
        }
    }

    /// Compare paths ignoring Windows verbatim prefixes, trailing slashes, and case on Windows.
    pub(crate) fn paths_equal(a: &str, b: &str) -> bool {
        codeterm_paths::paths_equal(a, b)
    }
}
