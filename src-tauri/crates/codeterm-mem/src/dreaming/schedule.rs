//! Schedule management for the bundled dreaming + legacy librarian crons.

use codeterm_types::config::MemoryConfig;

use crate::ports::{MemCronCreateParams, MemSchedulerPort};
use crate::worker_agent::{resolve_worker_agent, resolve_worker_model, BackgroundModelPort};

/// Marker name for the auto-installed dreaming cron job. Used to
/// detect "already exists" so we don't create duplicates on every start.
pub const DREAMING_CRON_NAME: &str = "mem-dreaming-auto";

/// The nightly optimizer task. `mem dream run` (phase=all) runs the SQL
/// phases + calibration and spawns the REM graph worker that writes the
/// daily_dream diary. Bare `mem dream` only prints help (no subcommand).
const DREAMING_TASK: &str =
    "codeterm mem dream run && codeterm agent report \"dreaming complete\"\n";

/// Create the bundled dreaming cron if it doesn't exist already.
/// - Schedule: "0 6 * * *" (6 AM)
/// - Jitter: 0..30 minutes delay (1_800_000 ms) — fires between 6:00 and 6:30 AM
/// - Task: runs `codeterm mem dream` in a fresh agent
/// - Workspace: "general" (the GA workspace)
///
/// Returns true if a new cron was created, false if one already existed.
pub fn ensure_dreaming_cron(
    cron_store: &mut dyn MemSchedulerPort,
    mem_cfg: &MemoryConfig,
    default_agent: &str,
    compat: &dyn BackgroundModelPort,
) -> Result<bool, String> {
    if let Some(job) = cron_store.find_job_by_name(DREAMING_CRON_NAME) {
        let desired_agent = resolve_worker_agent(None, &mem_cfg.dreaming_agent, default_agent);
        let agent_changed = job.agent != desired_agent;
        let legacy_schedule_match = matches!(job.schedule.as_str(), "0 3 * * *");
        let legacy_jitter_match = job.jitter_max_ms == Some(5_400_000);
        // Self-heal a stale command left by an older CLI shape (e.g. bare
        // `mem dream`) so existing installs don't silently no-op forever.
        let task_changed = job.task != DREAMING_TASK;

        if agent_changed || legacy_schedule_match || legacy_jitter_match || task_changed {
            let job_id = job.id.clone();
            let mut schedule = job.schedule.clone();
            let mut jitter = job.jitter_max_ms;
            let enabled = job.enabled;
            let oneshot = job.oneshot;
            let tz = job.tz.clone();
            let mut next_run = job.next_run;
            if legacy_schedule_match {
                schedule = "0 6 * * *".into();
            }
            if legacy_jitter_match {
                jitter = Some(1_800_000);
            }
            if enabled && !oneshot {
                match cron_store.compute_next_run_jittered_in(&schedule, jitter, tz.as_deref()) {
                    Ok(Some(nr)) => next_run = Some(nr),
                    Ok(None) => log::warn!(
                        "[dreaming] schedule {:?} has no upcoming occurrence; \
                         leaving next_run={:?} unchanged",
                        schedule,
                        next_run,
                    ),
                    Err(e) => log::warn!(
                        "[dreaming] recompute next_run after migration failed: {}; \
                         leaving next_run={:?} unchanged",
                        e,
                        next_run,
                    ),
                }
            }
            cron_store.update_job(&job_id, &mut |j| {
                if agent_changed {
                    j.agent = desired_agent.clone();
                }
                if task_changed {
                    j.task = DREAMING_TASK.to_string();
                }
                j.schedule = schedule.clone();
                j.jitter_max_ms = jitter;
                j.next_run = next_run;
            })?;
            let _ = cron_store.save();
        }
        return Ok(false);
    }

    // Deferred default: `None` here means "pick the default agent at fire
    // time", not "no agent" — deliberately NOT resolved eagerly, unlike the
    // update path above. Only used as a hint below for the model-compat
    // check, which does need to know which provider's catalog to consult.
    let agent = if mem_cfg.dreaming_agent.is_empty() {
        None
    } else {
        Some(mem_cfg.dreaming_agent.clone())
    };
    let provider_hint = resolve_worker_agent(None, &mem_cfg.dreaming_agent, default_agent);

    let (model, _fell_back) = resolve_worker_model(
        &mem_cfg.dreaming_model,
        &mem_cfg.worker_model,
        &provider_hint,
        compat,
    );
    let model = (!model.is_empty()).then_some(model);

    cron_store.create_job(MemCronCreateParams {
        name: DREAMING_CRON_NAME.into(),
        workspace_id: String::new(),
        schedule: "0 6 * * *".into(),
        task: DREAMING_TASK.into(),
        description: Some("Memory optimizer. Runs nightly as a graph worker inside Memory Panel. Toggle on/off in settings. Cannot be deleted.".into()),
        agent,
        model,
        cwd: None,
        role: None,
        creator_tab_id: None,
        enabled: Some(true),
        max_history: Some(30),
        oneshot: Some(false),
        jitter_max_ms: Some(1_800_000),
        tz: None,
        timeout_seconds: Some(mem_cfg.dreaming_timeout_seconds),
    })?;
    if let Some(job) = cron_store.find_job_by_name(DREAMING_CRON_NAME) {
        let job_id = job.id.clone();
        cron_store.set_job_protected(&job_id, true)?;
        let _ = cron_store.save();
    }
    Ok(true)
}

pub const LIBRARIAN_CRON_NAME: &str = "mem-librarian-auto";

/// Removes the legacy LLM-based librarian cron if it still exists from older
/// installs. The librarian now runs as a CLI subprocess via the supervisor;
/// kept as a migration step.
pub fn remove_legacy_librarian_cron(cron_store: &mut dyn MemSchedulerPort) -> bool {
    let existing = cron_store
        .find_job_by_name(LIBRARIAN_CRON_NAME)
        .map(|j| j.id.clone());
    if let Some(id) = existing {
        let _ = cron_store.set_job_protected(&id, false);
        let _ = cron_store.delete_job(&id);
        let _ = cron_store.save();
        true
    } else {
        false
    }
}
