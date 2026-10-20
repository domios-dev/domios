//! Where one report is delivered: local tab, mesh peer, or nobody.

/// Where a report should be delivered. A target is either a tab on this same
/// daemon, or a tab (id or `main`/`general` alias) on a paired mesh peer — the
/// latter lets a worker spawned via `agent spawn --mesh PEER` report back to the
/// real orchestrator / General target on another machine without the reporter
/// ever naming the peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportTarget {
    /// A tab on this daemon.
    Local(String),
    /// A tab id or alias on a paired peer, addressed by its instance name/id.
    Mesh { peer: String, tab: String },
}

impl ReportTarget {
    /// Stable key the report store and worker-death dedup index by. Local targets
    /// key on the tab id; mesh targets
    /// key on a synthetic `mesh:<peer>/<tab>` so they neither collide with local
    /// tabs nor each other.
    pub fn storage_key(&self) -> String {
        match self {
            ReportTarget::Local(id) => id.clone(),
            ReportTarget::Mesh { peer, tab } => format!("mesh:{}/{}", peer, tab),
        }
    }
}

/// True when the caller passed an explicit plan id that `resolve_plan_ctx` did
/// NOT resolve to the same local plan — i.e., the plan is on a different peer's
/// store, or `resolve_plan_ctx` silently substituted a local binding. When this
/// returns true, report homing should use the explicit hint, NOT the local
/// substitution.
pub(crate) fn is_explicit_plan_foreign(
    explicit_plan_id: Option<&str>,
    eff_plan_id: Option<&str>,
) -> bool {
    explicit_plan_id
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| eff_plan_id.map(str::trim) != Some(p))
        .unwrap_or(false)
}

/// Resolve where a report from `tab_id` should be delivered.
///
/// Returns `(from_title, Some(target))` when a real target exists, or
/// `(from_title, None)` when the report has no implicit recipient and must fail
/// with an explicit `codeterm send` hint. The resolution order is:
///
/// 1. `report_target_mesh_peer` (+ tab/alias) — explicit remote target set by
///    `agent spawn --mesh PEER --report-to ...`; wins so a remote worker reports
///    across the mesh, not into a stale local field.
/// 2. `parent_tab_id` — direct parent in the role hierarchy (cascades up)
/// 3. `report_target_tab_id` — cron / mem worker non-hierarchy target
/// 4. `spawned_by_tab_id` — agent that ran `agent spawn`, even when no
///    role parent (top-level Orchestrator spawned by another agent)
///
/// The reporter's own tab id is filtered out at every local level: a tab must
/// never resolve to itself, otherwise reports get injected into the reporter's
/// own buffer and never reach the spawning agent.
pub fn resolve_report_target(
    mux: &crate::multiplexer::state::MultiplexerState,
    tab_id: &str,
) -> Result<(String, Option<ReportTarget>), String> {
    let tab = mux.get_tab(tab_id).ok_or("Tab not found")?;
    let title = mux.display_label_for_tab(tab_id);

    let nonempty = |s: &Option<String>| {
        s.as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    if let (Some(peer), Some(target_tab)) = (
        nonempty(&tab.report_target_mesh_peer),
        nonempty(&tab.report_target_tab_id),
    ) {
        return Ok((
            title,
            Some(ReportTarget::Mesh {
                peer,
                tab: target_tab,
            }),
        ));
    }

    let local = [
        tab.parent_tab_id.as_deref(),
        tab.report_target_tab_id.as_deref(),
        tab.spawned_by_tab_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    .find(|candidate| !candidate.is_empty() && *candidate != tab_id)
    .map(|id| ReportTarget::Local(id.to_string()));
    Ok((title, local))
}

/// Resolve a paired peer by instance name (case-insensitive) or instance id and
/// build a signed transport client for it. Name first so the human-facing
/// `--mesh Mac-Air` works; falls back to a raw instance id. When several
/// paired records share a name (a peer that re-minted its instance_id left a
/// dead duplicate behind, codeterm#434), prefers the online record with the
/// highest relay protocol version deterministically instead of whichever the
/// hashmap iterates first.
pub(super) fn resolve_peer_client_by_name(
    mesh: &crate::mesh::manager::MeshManager,
    name: &str,
) -> Option<crate::mesh::transport::PeerClient> {
    let peer =
        crate::mesh::paired_resolve::resolve_paired_by_name(&mesh.paired_peers, name).ok()?;
    mesh.client_for(&peer.instance_id)
}

/// Whether a dying leaf tab needs a synthetic death report, and to whom.
/// Returns `(target, message)` when the tab has not reported to that target.
pub fn worker_death_report(
    mux: &crate::multiplexer::state::MultiplexerState,
    report_store: &crate::agent::reports::ReportStore,
    tab_id: &str,
) -> Option<(String, String)> {
    let role_label = match mux.get_tab(tab_id)?.role {
        Some(crate::multiplexer::types::AgentRole::Manager) => "MANAGER",
        Some(crate::multiplexer::types::AgentRole::Worker) => "WORKER",
        _ => "WORKER",
    };
    let (_from_title, target) = resolve_report_target(mux, tab_id).ok()?;
    let target = target?;
    let storage_key = target.storage_key();
    if report_store.has_report_from(&storage_key, tab_id) {
        return None;
    }
    Some((
        storage_key,
        format!("{role_label} {} EXITED without reporting (dead)", tab_id),
    ))
}
