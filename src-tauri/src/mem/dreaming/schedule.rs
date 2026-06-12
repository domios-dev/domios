//! Schedule management for the bundled dreaming + legacy librarian crons.

/// Marker name for the auto-installed dreaming cron job. Used to
/// detect "already exists" so we don't create duplicates on every start.
pub const DREAMING_CRON_NAME: &str = "mem-dreaming-auto";

/// The nightly optimizer task. First runs SQL phases (light + deep),
/// then spawns a graph worker for intelligent optimization.
const DREAMING_TASK: &str = "codeterm mem dream && codeterm agent report \"dreaming complete\"\n";

/// Create the bundled dreaming cron if it doesn't exist already.
/// - Schedule: "0 6 * * *" (6 AM)
/// - Jitter: 0..30 minutes delay (1_800_000 ms) — fires between 6:00 and 6:30 AM
/// - Task: runs `codeterm mem dream` in a fresh agent
/// - Workspace: "general" (the GA workspace)
///
/// Returns true if a new cron was created, false if one already existed.
pub fn ensure_dreaming_cron(
    cron_store: &mut crate::cron::store::CronStore,
) -> Result<bool, String> {
    let mem_cfg = crate::config::memory_config();

    if let Some(job) = cron_store
        .list()
        .iter()
        .find(|j| j.name == DREAMING_CRON_NAME)
    {
        let desired_agent = if mem_cfg.dreaming_agent.is_empty() {
            crate::config::default_agent_provider()
        } else {
            mem_cfg.dreaming_agent.clone()
        };
        let agent_changed = job.agent != desired_agent;
        let legacy_schedule_match = matches!(job.schedule.as_str(), "0 3 * * *");
        let legacy_jitter_match = job.jitter_max_ms == Some(5_400_000);

        if agent_changed || legacy_schedule_match || legacy_jitter_match {
            let job_id = job.id.clone();
            if let Some(j) = cron_store.get_mut(&job_id) {
                if agent_changed {
                    j.agent = desired_agent;
                }
                if legacy_schedule_match {
                    j.schedule = "0 6 * * *".into();
                }
                if legacy_jitter_match {
                    j.jitter_max_ms = Some(1_800_000);
                }
                // Migrated schedule: stale `next_run` can cause an extra run on startup.
                // Recompute to avoid double-fire after migration.
                if j.enabled && !j.oneshot {
                    // Only overwrite next_run when compute returns Some(_). None means no future run exists.
                    // Keeping old next_run avoids accidentally disabling jobs (issue #96).
                    match crate::cron::store::compute_next_run_jittered_in(
                        &j.schedule,
                        j.jitter_max_ms,
                        j.tz.as_deref(),
                    ) {
                        Ok(Some(nr)) => j.next_run = Some(nr),
                        Ok(None) => log::warn!(
                            "[dreaming] schedule {:?} has no upcoming occurrence; \
                             leaving next_run={:?} unchanged",
                            j.schedule,
                            j.next_run,
                        ),
                        Err(e) => log::warn!(
                            "[dreaming] recompute next_run after migration failed: {}; \
                             leaving next_run={:?} unchanged",
                            e,
                            j.next_run,
                        ),
                    }
                }
                let _ = cron_store.save();
            }
        }
        return Ok(false);
    }

    let agent = if mem_cfg.dreaming_agent.is_empty() {
        None
    } else {
        Some(mem_cfg.dreaming_agent.clone())
    };

    // Prefer `dreaming_model` override; otherwise store shared `worker_model` for manual-run parity.
    let model = if !mem_cfg.dreaming_model.is_empty() {
        Some(mem_cfg.dreaming_model.clone())
    } else if !mem_cfg.worker_model.is_empty() {
        Some(mem_cfg.worker_model.clone())
    } else {
        None
    }
    .map(|s| crate::agent::models::canonicalize_short(&s).to_string());

    let _ = cron_store.create(crate::cron::store::CronCreateParams {
        name: DREAMING_CRON_NAME.into(),
        workspace_id: String::new(),
        schedule: "0 6 * * *".into(),
        task: DREAMING_TASK.into(),
        description: Some("Memory optimizer. Runs nightly as a graph worker inside Memory Panel. Toggle on/off in settings. Cannot be deleted.".into()),
        agent,
        model,
        cwd: None,
        role: None,
        creator_pane_id: None,
        enabled: Some(true),
        max_history: Some(30),
        oneshot: Some(false),
        jitter_max_ms: Some(1_800_000),
        tz: None,
        timeout_seconds: Some(mem_cfg.dreaming_timeout_seconds),
    })?;
    // Mark as protected after creation
    if let Some(job) = cron_store
        .list()
        .iter()
        .find(|j| j.name == DREAMING_CRON_NAME)
    {
        let job_id = job.id.clone();
        if let Some(j) = cron_store.get_mut(&job_id) {
            j.protected = true;
        }
        let _ = cron_store.save();
    }
    Ok(true)
}

pub const LIBRARIAN_CRON_NAME: &str = "mem-librarian-auto";

/// Removes the legacy LLM-based librarian cron if it still exists from older
/// installs. The librarian now runs as a CLI subprocess via the supervisor;
/// kept as a migration step.
pub fn remove_legacy_librarian_cron(cron_store: &mut crate::cron::store::CronStore) -> bool {
    let existing = cron_store
        .list()
        .iter()
        .find(|j| j.name == LIBRARIAN_CRON_NAME)
        .map(|j| j.id.clone());
    if let Some(id) = existing {
        if let Some(job) = cron_store.get_mut(&id) {
            job.protected = false;
        }
        let _ = cron_store.delete(&id);
        let _ = cron_store.save();
        true
    } else {
        false
    }
}
