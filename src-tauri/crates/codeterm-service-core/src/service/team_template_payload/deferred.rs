//! Payload items that land after start. A Blueprint imports under a live, authenticated
//! Orchestrator, so it waits for that Orchestrator's readiness events; a skill is a plan skill,
//! so it waits Team-scoped on disk until the Team's plan exists.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use parking_lot::Mutex;

use super::{automation_port, AutomationItem, SkillFolder};
use crate::service::ServiceContext;

/// Blueprints accepted at start, by Team, until its Orchestrator is live.
fn waiting() -> &'static Mutex<BTreeMap<String, Vec<AutomationItem>>> {
    static WAITING: OnceLock<Mutex<BTreeMap<String, Vec<AutomationItem>>>> = OnceLock::new();
    WAITING.get_or_init(Mutex::default)
}

/// Whether any Team may hold skills on disk; starts true so the first plan change looks.
static HELD: AtomicBool = AtomicBool::new(true);

/// The Team's live local Orchestrator, when it can take a Blueprint now.
/// A Blueprint's creator must be the Team's local anchor whose current launch proved itself.
pub fn live_orchestrator(svc: &ServiceContext, team_id: &str) -> Option<String> {
    let mux = svc.mux.read();
    mux.team_member_tab_ids(team_id).into_iter().find(|id| {
        mux.get_tab(id).is_some_and(|tab| {
            tab.anchors_team()
                && tab.has_team_orchestrator_authority()
                && tab.launch_session_authenticated()
                && tab.report_target_mesh_peer.is_none()
        })
    })
}

/// Queues a Blueprint for the Team, then settles at once in case the Orchestrator went live
/// between the caller's check and the queueing.
pub(super) fn wait_for_orchestrator(svc: &ServiceContext, team_id: &str, item: AutomationItem) {
    waiting()
        .lock()
        .entry(team_id.to_string())
        .or_default()
        .push(item);
    settle_team(svc, team_id);
}

/// Imports a Team's waiting Blueprints when its Orchestrator is live; each is taken once.
/// Blocking: runs the Blueprint authority.
fn settle_team(svc: &ServiceContext, team_id: &str) {
    let Some(orchestrator) = live_orchestrator(svc, team_id) else {
        return;
    };
    let Some(items) = waiting().lock().remove(team_id) else {
        return;
    };
    let failures: Vec<String> = match automation_port() {
        None => items
            .iter()
            .map(|item| {
                format!(
                    "automation:{}: Blueprint automation is not available",
                    item.name
                )
            })
            .collect(),
        Some(port) => items
            .iter()
            .filter_map(|item| {
                port.apply(team_id, item)
                    .err()
                    .map(|error| format!("automation:{}: {error}", item.name))
            })
            .collect(),
    };
    log::info!(
        "[template-start] team={team_id} imported {} waiting Blueprint(s), {} failed",
        items.len() - failures.len(),
        failures.len()
    );
    if failures.is_empty() {
        return;
    }
    let text = format!(
        "Template start: these Blueprints could not be imported for Team {team_id}:\n- {}",
        failures.join("\n- ")
    );
    if let Err(error) = crate::service::messenger::Messenger::deliver(
        svc,
        &orchestrator,
        crate::service::messenger::MessageEnvelope::codeterm_reminder("template-start"),
        &text,
    ) {
        log::warn!("[template-start] team={team_id} Blueprint failure not delivered: {error}");
    }
}

/// A readiness event for `tab_id` (authenticated launch, agent ready): settles the Blueprints
/// its Team is waiting on, off the caller's thread. Cheap when nothing waits.
pub fn settle_automation_for_tab(svc: &ServiceContext, tab_id: &str) {
    if waiting().lock().is_empty() {
        return;
    }
    let team = svc
        .mux
        .read()
        .get_tab(tab_id)
        .filter(|tab| tab.anchors_team())
        .and_then(|tab| tab.team_id.clone());
    let Some(team) = team.filter(|team| waiting().lock().contains_key(team)) else {
        return;
    };
    let svc = svc.clone();
    drop(crate::async_runtime::spawn_blocking(move || {
        settle_team(&svc, &team)
    }));
}

/// A closed Team's waiting Blueprints go with it.
pub(super) fn forget_waiting(team_id: &str) {
    waiting().lock().remove(team_id);
}

/// Where a Team keeps the package skills its plan has not taken yet.
pub(super) fn held_skills_dir(team_id: &str) -> PathBuf {
    crate::paths::data_dir()
        .join("teams")
        .join(team_id)
        .join("skills")
}

/// Keeps a skill Team-scoped until the Team's plan binds.
pub(super) fn hold_skill(team_id: &str, skill: &SkillFolder) -> Result<(), String> {
    let dir = super::service_port::under(&held_skills_dir(team_id), &skill.name)?;
    for (path, bytes) in &skill.files {
        super::service_port::write_under(&dir, path, bytes)?;
    }
    HELD.store(true, Ordering::SeqCst);
    Ok(())
}

/// The skills a Team holds, by folder name.
pub(super) fn held_skills(team_id: &str) -> Result<Vec<SkillFolder>, String> {
    let root = held_skills_dir(team_id);
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Ok(Vec::new());
    };
    let mut skills = Vec::new();
    for entry in entries.flatten() {
        let metadata = std::fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
        if !metadata.is_dir() || codeterm_util::safe_tree::is_link(&metadata) {
            continue;
        }
        skills.push(SkillFolder {
            name: entry.file_name().to_string_lossy().into_owned(),
            description: String::new(),
            files: super::service_port::read_tree(&entry.path())?,
        });
    }
    skills.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(skills)
}

fn any_team_holds_skills(teams: &Path) -> bool {
    std::fs::read_dir(teams)
        .into_iter()
        .flatten()
        .flatten()
        .any(|team| {
            std::fs::read_dir(team.path().join("skills"))
                .is_ok_and(|mut skills| skills.next().is_some())
        })
}

/// Moves a Team's held skills into its plan. Blocking.
fn attach_held_skills(svc: &ServiceContext, team_id: &str, plan_id: &str) {
    let held = match held_skills(team_id) {
        Ok(held) => held,
        Err(error) => {
            log::warn!("[template-start] team={team_id} held skills unreadable: {error}");
            return;
        }
    };
    for skill in held {
        match super::service_port::install_plan_skill(svc, plan_id, &skill) {
            Ok(()) => {
                let dir = held_skills_dir(team_id).join(&skill.name);
                if let Err(error) = std::fs::remove_dir_all(&dir) {
                    log::warn!(
                        "[template-start] held skill {} not cleared: {error}",
                        skill.name
                    );
                }
            }
            Err(error) => log::warn!(
                "[template-start] team={team_id} skill {} not attached to {plan_id}: {error}",
                skill.name
            ),
        }
    }
}

/// A plan changed: when it is bound to a Team that holds skills, they join that plan.
/// Cheap when no Team holds any; the work runs off the caller's thread.
pub fn attach_held_skills_for_plan(svc: &ServiceContext, plan_id: &str) {
    if !HELD.load(Ordering::SeqCst) {
        return;
    }
    let svc = svc.clone();
    let plan_id = plan_id.to_string();
    drop(crate::async_runtime::spawn_blocking(move || {
        HELD.store(false, Ordering::SeqCst);
        let team = svc.plan_store.plan_team_id(&plan_id).flatten();
        if let Some(team) = team {
            if svc.plan_store.plan_id_for_team(&team).as_deref() == Some(plan_id.as_str()) {
                attach_held_skills(&svc, &team, &plan_id);
            }
        }
        if any_team_holds_skills(&crate::paths::data_dir().join("teams")) {
            HELD.store(true, Ordering::SeqCst);
        }
    }));
}
