//! App-side implementations of [`crate::mem::ports`] traits.

#[cfg(test)]
mod dreaming_cron_tests;
pub(crate) mod worker_spawn_spec;

use std::collections::HashMap;
use std::sync::Arc;

use crate::cron::store::{self, CronCreateParams, CronStore};
use crate::mem::ports::{
    MemCronCreateParams, MemCronJob, MemEventSink, MemHostContext, MemSchedulerPort,
};
use crate::mem::workers::{self, GraphWorkerInfo, WorkerStatus};
use crate::multiplexer::types::TabCloseReason;
use crate::service::ServiceContext;

fn cron_to_mem(job: &store::CronJob) -> MemCronJob {
    MemCronJob {
        id: job.id.clone(),
        name: job.name.clone(),
        agent: job.agent.clone(),
        schedule: job.schedule.clone(),
        task: job.task.clone(),
        enabled: job.enabled,
        oneshot: job.oneshot,
        next_run: job.next_run,
        jitter_max_ms: job.jitter_max_ms,
        tz: job.tz.clone(),
        protected: job.protected,
    }
}

/// Legacy JSON edits apply in memory until `save`; Storage edits collect in a draft that
/// `cron::store::settle_port` commits once the caller released the cron lock.
pub struct CronSchedulerPort<'a>(pub &'a mut CronStore);

impl CronSchedulerPort<'_> {
    fn job(&self, id: &str) -> Result<store::CronJob, String> {
        self.0
            .port_jobs()
            .iter()
            .find(|job| job.id == id)
            .cloned()
            .ok_or_else(|| format!("cron job not found: {id}"))
    }
}

impl MemSchedulerPort for CronSchedulerPort<'_> {
    fn list_jobs(&self) -> Vec<MemCronJob> {
        self.0.port_jobs().iter().map(cron_to_mem).collect()
    }

    fn find_job_by_name(&self, name: &str) -> Option<MemCronJob> {
        self.0
            .port_jobs()
            .iter()
            .find(|j| j.name == name)
            .map(cron_to_mem)
    }

    fn update_job(
        &mut self,
        id: &str,
        update: &mut dyn FnMut(&mut MemCronJob),
    ) -> Result<(), String> {
        let mut job = self.job(id)?;
        let mut view = cron_to_mem(&job);
        update(&mut view);
        job.agent = view.agent;
        job.schedule = view.schedule;
        job.task = view.task;
        job.jitter_max_ms = view.jitter_max_ms;
        job.next_run = view.next_run;
        job.enabled = view.enabled;
        job.oneshot = view.oneshot;
        job.protected = view.protected;
        job.tz = view.tz;
        self.0.port_update(job)
    }

    fn set_job_protected(&mut self, id: &str, protected: bool) -> Result<(), String> {
        let mut job = self.job(id)?;
        job.protected = protected;
        self.0.port_update(job)
    }

    fn create_job(&mut self, params: MemCronCreateParams) -> Result<(), String> {
        self.0.port_create(CronCreateParams {
            name: params.name,
            workspace_id: params.workspace_id,
            team_id: None,
            detach_from_team: false,
            schedule: params.schedule,
            task: params.task,
            description: params.description,
            agent: params.agent,
            model: params.model,
            reasoning_effort: None,
            cwd: params.cwd,
            role: params.role,
            skill: None,
            creator_tab_id: params.creator_tab_id,
            report_target_tab_id: None,
            enabled: params.enabled,
            max_history: params.max_history,
            oneshot: params.oneshot,
            jitter_max_ms: params.jitter_max_ms,
            tz: params.tz,
            timeout_seconds: params.timeout_seconds,
        })
    }

    fn delete_job(&mut self, id: &str) -> Result<(), String> {
        self.0.port_delete(id)
    }

    fn save(&mut self) -> Result<(), String> {
        self.0.port_save()
    }

    fn compute_next_run_jittered_in(
        &self,
        schedule: &str,
        jitter_max_ms: Option<u64>,
        tz: Option<&str>,
    ) -> Result<Option<u64>, String> {
        store::compute_next_run_jittered_in(schedule, jitter_max_ms, tz)
    }
}

struct AppMemEventSink(crate::app_runtime::AppHandle);

impl MemEventSink for AppMemEventSink {
    fn emit_graph_worker_spawned(&self, info: &GraphWorkerInfo) {
        codeterm_events::emit_everywhere(&self.0, "graph-worker-spawned", info);
    }
}

impl MemHostContext for ServiceContext {
    fn mem(&self) -> Arc<crate::mem::MemManager> {
        self.mem.clone()
    }

    fn get_taxonomy(
        &self,
        workspace: Option<&str>,
    ) -> Result<HashMap<String, HashMap<String, u64>>, String> {
        self.mem.get_taxonomy(workspace)
    }

    fn query_graph_type_counts(&self) -> (Vec<(String, u64)>, Vec<(String, u64)>) {
        let entity_types: Vec<(String, u64)> = {
            let conn = self.mem.conn().lock();
            conn.prepare(
                "SELECT entity_type, COUNT(*) FROM entities GROUP BY entity_type ORDER BY COUNT(*) DESC LIMIT 20",
            )
            .and_then(|mut stmt| {
                stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?)))
                    .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
            })
            .unwrap_or_default()
        };

        let relation_types: Vec<(String, u64)> = {
            let conn = self.mem.conn().lock();
            conn.prepare(
                "SELECT relation_type, COUNT(*) FROM relations GROUP BY relation_type ORDER BY COUNT(*) DESC LIMIT 30",
            )
            .and_then(|mut stmt| {
                stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?)))
                    .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
            })
            .unwrap_or_default()
        };

        (entity_types, relation_types)
    }
}

pub fn spawn_headless_worker(
    svc: Arc<ServiceContext>,
    params: workers::MemWorkerSpawnParams,
) -> Result<String, String> {
    spawn_worker(svc, params, None)
}

fn spawn_worker(
    svc: Arc<ServiceContext>,
    params: workers::MemWorkerSpawnParams,
    extraction_fingerprint: Option<String>,
) -> Result<String, String> {
    let workers::MemWorkerSpawnParams {
        intent,
        task_prompt,
        spawned_by,
        agent_provider,
        model,
    } = params;

    if let Some(existing) = svc.workers.active_by_intent(&intent) {
        log::info!(
            "[worker] skip spawn for intent '{}': {} already running",
            intent,
            existing
        );
        return Ok(existing);
    }

    let worker_id = workers::new_worker_id();
    let color = svc.workers.pick_color();
    let mut info = workers::make_info(
        worker_id.clone(),
        String::new(),
        intent.clone(),
        spawned_by.clone(),
        color,
    );
    info.status = WorkerStatus::Queued;
    info.agent_provider = Some(agent_provider.clone());
    if !model.is_empty() {
        info.model = Some(model.clone());
    }
    if let Some(fingerprint) = extraction_fingerprint {
        let Some(retry) = svc.workers.extraction_retries.claim(
            &intent,
            &fingerprint,
            &worker_id,
            info.spawned_at,
        )?
        else {
            return Err("extraction is completed, active, backing off, or parked; use mem extract-pending to retry".into());
        };
        info.extraction_retry = Some(retry);
    }
    svc.workers.register(info.clone());
    AppMemEventSink(svc.app_handle.clone()).emit_graph_worker_spawned(&info);

    let svc_task = svc.clone();
    let worker_id_task = worker_id.clone();
    tauri::async_runtime::spawn(async move {
        let permit = svc_task.worker_pool.acquire(&agent_provider).await;
        if svc_task
            .workers
            .get(&worker_id_task)
            .map(|w| w.status.is_terminal())
            .unwrap_or(true)
        {
            drop(permit);
            return;
        }
        let _ = svc_task
            .workers
            .update_status(&worker_id_task, WorkerStatus::Starting);

        let task = workers::build_worker_task(&intent, &worker_id_task, &task_prompt);
        let svc_blocking = svc_task.clone();
        let worker_id_blocking = worker_id_task.clone();
        let agent_for_spawn = agent_provider.clone();
        let model_for_spawn = model.clone();
        let spawned_by_for_spawn = spawned_by.clone();
        let intent_for_spawn = intent.clone();
        let intent_title = intent_for_spawn.clone();
        let spawn_result = tokio::task::spawn_blocking(move || {
            let worker_cwd = crate::paths::data_dir().join("workers");
            std::fs::create_dir_all(&worker_cwd)
                .map_err(|error| format!("create worker directory: {error}"))?;
            let spec = worker_spawn_spec::build_mem_worker_spawn_spec(
                svc_blocking.chatbackend_registry.as_ref(),
                worker_cwd,
                worker_spawn_spec::MemWorkerSpawnInputs {
                    agent_provider: agent_for_spawn,
                    model: model_for_spawn,
                    task,
                    worker_id: worker_id_blocking,
                    spawned_by: spawned_by_for_spawn,
                    intent: intent_title,
                    instance_cli_path: Some(
                        crate::paths::data_dir()
                            .join("bin")
                            .join(format!("codeterm{}", std::env::consts::EXE_SUFFIX))
                            .into_os_string(),
                    ),
                    inherited_path: std::env::var_os("PATH").unwrap_or_default(),
                },
            )?;
            svc_blocking.spawn_tab(spec)
        })
        .await;

        let handle = match spawn_result {
            Ok(Ok(h)) => h,
            Ok(Err(e)) => {
                log::error!(
                    "[worker] {}/{} spawn_tab failed: {}",
                    intent_for_spawn,
                    worker_id_task,
                    e
                );
                let _ = svc_task.workers.finish_execution(
                    &worker_id_task,
                    None,
                    codeterm_mem::dream_status::ExecutionEnd::ProcessEnded {
                        detail: format!("spawn_tab failed: {e}"),
                    },
                );
                drop(permit);
                return;
            }
            Err(e) => {
                log::error!(
                    "[worker] {}/{} spawn_blocking panicked: {}",
                    intent_for_spawn,
                    worker_id_task,
                    e
                );
                let _ = svc_task.workers.finish_execution(
                    &worker_id_task,
                    None,
                    codeterm_mem::dream_status::ExecutionEnd::ProcessEnded {
                        detail: format!("spawn task failed: {e}"),
                    },
                );
                drop(permit);
                return;
            }
        };

        svc_task
            .workers
            .set_tab_id(&worker_id_task, handle.tab_id.clone());
        log::info!(
            "[worker] headless {} tab={} pty={}",
            worker_id_task,
            handle.tab_id,
            handle.pty_id.as_deref().unwrap_or("?")
        );

        let memory_config = svc_task.configuration().memory;
        let run_deadline_secs =
            crate::mem::worker_deadline::worker_deadline_secs(&intent_for_spawn, &memory_config);
        let watchdog_deadline_secs =
            crate::mem::worker_deadline::worker_watchdog_deadline_secs(run_deadline_secs);
        let watchdog_grace_secs = watchdog_deadline_secs - run_deadline_secs;
        let watchdog_detail = format!(
            "worker watchdog deadline {}s (run deadline {}s + {}s grace)",
            watchdog_deadline_secs, run_deadline_secs, watchdog_grace_secs
        );
        let deadline =
            tokio::time::Instant::now() + std::time::Duration::from_secs(watchdog_deadline_secs);
        if tokio::time::timeout_at(deadline, svc_task.workers.wait_terminal(&worker_id_task))
            .await
            .is_err()
        {
            log::warn!(
                    "[worker] {}/{} exceeded {}s watchdog (run deadline {}s + {}s grace), recording timeout + closing Tab",
                    intent_for_spawn,
                    worker_id_task,
                    watchdog_deadline_secs,
                    run_deadline_secs,
                    watchdog_grace_secs
                );
            let _ = svc_task.workers.finish_execution(
                &worker_id_task,
                None,
                codeterm_mem::dream_status::ExecutionEnd::TimedOut {
                    detail: watchdog_detail.clone(),
                },
            );
            let _ = svc_task.close_tab_with_reason(&handle.tab_id, TabCloseReason::Watchdog);
            let (svc_end, worker_end) = (svc_task.clone(), worker_id_task.clone());
            let _ = tokio::task::spawn_blocking(move || {
                crate::mem::dream_runs::finalize_dream_at_deadline(
                    &svc_end,
                    &worker_end,
                    &watchdog_detail,
                )
            })
            .await;
        }
        drop(permit);
    });

    Ok(worker_id)
}

pub fn spawn_headless(
    svc: &Arc<ServiceContext>,
    intent: String,
    task_prompt: String,
    spawned_by: String,
    agent_provider: String,
    model: String,
) -> Result<String, String> {
    spawn_headless_worker(
        svc.clone(),
        workers::MemWorkerSpawnParams {
            intent,
            task_prompt,
            spawned_by,
            agent_provider,
            model,
        },
    )
}

pub fn spawn_extraction_worker(
    svc: &Arc<ServiceContext>,
    mem_cfg: &codeterm_types::config::MemoryConfig,
    default_agent: &str,
    mem_item_id: String,
    title: String,
    body: String,
    kind: codeterm_mem::store::MemKind,
) -> Option<String> {
    if !codeterm_mem::auto_extract::should_extract_public(kind) {
        return None;
    }
    let prompt = codeterm_mem::auto_extract::build_extraction_prompt(
        svc.as_ref(),
        mem_cfg,
        &mem_item_id,
        &title,
        &body,
    );
    let resolved = codeterm_mem::worker_agent::resolve_worker(
        None,
        &mem_cfg.extraction_agent,
        &mem_cfg.extraction_model,
        default_agent,
        &mem_cfg.worker_model,
        &crate::service::spawn::CatalogBackgroundModelPort,
    );
    let agent_provider = resolved.agent;
    let model = resolved.model;
    let intent = format!("extract:{mem_item_id}");
    if let Some(worker_id) = svc.workers.active_by_intent(&intent) {
        return Some(worker_id);
    }

    // A source that vanished before we got here has no row to bind, so there is nothing to authorize and no worker worth spawning.
    let Some(source_uid) = source_uid_of(svc, &mem_item_id) else {
        log::info!(
            "[auto-extract] source {} is gone before spawn, skipping",
            mem_item_id
        );
        return None;
    };
    let generation = svc
        .mem
        .extraction_authority()
        .open(&mem_item_id, &source_uid);

    let availability = crate::service::spawn::provider_launch_preflight(
        &svc.agent_registry,
        &agent_provider,
        true,
    )
    .is_ok()
    .to_string();
    let fingerprint = codeterm_mem::extraction_retry::fingerprint(&[
        &source_uid,
        &title,
        &body,
        &agent_provider,
        &model,
        &availability,
        codeterm_build_info::BUILD_SHA,
    ]);
    match spawn_worker(
        svc.clone(),
        workers::MemWorkerSpawnParams {
            intent,
            task_prompt: prompt,
            spawned_by: "auto:save".into(),
            agent_provider,
            model,
        },
        Some(fingerprint),
    ) {
        Ok(worker_id) => {
            let authority = svc.mem.extraction_authority();
            if authority.bind_worker(&generation, &worker_id)
                == codeterm_mem::BindOutcome::AlreadyRevoked
            {
                // The delete landed between `open` and here; the revocation could
                // not name this worker, so cancel it on the bind result itself.
                log::info!(
                    "[auto-extract] generation {} was revoked during spawn, cancelling worker {}",
                    generation.token(),
                    worker_id
                );
                crate::api::mem_routes::cancel_worker(svc, &worker_id);
                return None;
            }
            log::info!(
                "[auto-extract] headless worker {} for {} generation {}",
                worker_id,
                mem_item_id,
                generation.token()
            );
            Some(worker_id)
        }
        Err(e) => {
            svc.mem.extraction_authority().close(&generation);
            log::warn!("[auto-extract] spawn failed for {}: {}", mem_item_id, e);
            None
        }
    }
}

fn source_uid_of(svc: &Arc<ServiceContext>, id: &str) -> Option<String> {
    svc.mem.get(id).ok().flatten().map(|item| item.uid)
}

/// Cancels exactly the Workers whose extraction generation lost authority.
/// Registered on the mem store so a delete anywhere revokes and cancels through
/// one path instead of each delete route re-implementing it.
pub struct WorkerCancelSink(pub Arc<ServiceContext>);

impl codeterm_mem::ExtractionCancelSink for WorkerCancelSink {
    fn cancel(&self, revoked: &[codeterm_mem::RevokedGeneration]) {
        for r in revoked {
            let Some(worker_id) = r.worker_id.as_deref() else {
                continue;
            };
            crate::api::mem_routes::cancel_worker(&self.0, worker_id);
        }
    }
}

pub fn run_extract_pending(svc: Arc<ServiceContext>, limit: usize) -> usize {
    extract_pending(svc, limit, true)
}

pub fn resume_extract_pending(svc: Arc<ServiceContext>, limit: usize) -> usize {
    extract_pending(svc, limit, false)
}

fn extract_pending(svc: Arc<ServiceContext>, limit: usize, explicit: bool) -> usize {
    let mem_cfg = svc.configuration().memory;
    let default_agent = svc.configuration().general_agent.agent_provider;
    let mut pending = match codeterm_mem::auto_extract::find_pending_items(&svc.mem, limit) {
        Ok(p) => p,
        Err(e) => {
            log::error!("[auto-extract] find_pending_items failed: {}", e);
            return 0;
        }
    };
    if explicit {
        match svc.workers.extraction_retries.failed_intents() {
            Ok(intents) => {
                for intent in intents {
                    let Some(id) = intent.strip_prefix("extract:") else {
                        continue;
                    };
                    if pending.len() >= limit
                        || pending.iter().any(|(existing, _, _, _)| existing == id)
                    {
                        continue;
                    }
                    if let Ok(Some(item)) = svc.mem.get(id) {
                        if let Some(kind) = codeterm_mem::store::MemKind::from_str(&item.kind) {
                            pending.push((item.id, item.title, item.body, kind));
                        }
                    }
                }
            }
            Err(error) => log::error!("[auto-extract] failed item lookup: {error}"),
        }
    }
    let count = pending.len();
    log::info!(
        "[auto-extract] queued {} pending items for extraction",
        count
    );
    let mem = svc.mem.clone();
    let fut = async move {
        let _permit = mem.acquire_heavy_permit().await;
        for (id, title, body, kind) in pending {
            if explicit
                && svc
                    .workers
                    .active_by_intent(&format!("extract:{id}"))
                    .is_none()
            {
                if let Err(error) = svc
                    .workers
                    .extraction_retries
                    .reset(&format!("extract:{id}"))
                {
                    log::error!("[auto-extract] reset failed: {error}");
                    continue;
                }
            }
            let (title, body) = svc
                .mem
                .get(&id)
                .ok()
                .flatten()
                .map(|item| (item.title, item.body))
                .unwrap_or((title, body));
            spawn_extraction_worker(&svc, &mem_cfg, &default_agent, id, title, body, kind);
        }
    };
    match tokio::runtime::Handle::try_current() {
        Ok(h) => {
            h.spawn(fut);
        }
        Err(_) => {
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("build extract runtime");
                rt.block_on(fut);
            });
        }
    }
    count
}

pub fn spawn_rem_worker(
    svc: &Arc<ServiceContext>,
    mem_cfg: &codeterm_types::config::MemoryConfig,
    default_agent: &str,
) {
    use codeterm_mem::dreaming::{DREAMING_TELEGRAM_STEP_DISABLED, DREAMING_TELEGRAM_STEP_ENABLED};
    use codeterm_mem::workers::DREAMING_INTENT_PREFIX;

    let resolved = codeterm_mem::worker_agent::resolve_worker(
        None,
        &mem_cfg.dreaming_agent,
        &mem_cfg.dreaming_model,
        default_agent,
        &mem_cfg.worker_model,
        &crate::service::spawn::CatalogBackgroundModelPort,
    );
    let agent = resolved.agent;
    let model = resolved.model;
    let telegram_step = if mem_cfg.dreaming_telegram_notify {
        DREAMING_TELEGRAM_STEP_ENABLED
    } else {
        DREAMING_TELEGRAM_STEP_DISABLED
    };
    let prompt =
        codeterm_mem::prompt_text::DREAMING_REM.replace("{telegram_notify_step}", telegram_step);
    match spawn_headless(
        svc,
        format!("{DREAMING_INTENT_PREFIX}rem"),
        prompt,
        "system:dreaming".into(),
        agent,
        model,
    ) {
        Ok(wid) => log::info!("[dreaming] REM worker spawned: {}", wid),
        Err(e) => log::error!("[dreaming] REM worker spawn failed: {}", e),
    }
}
