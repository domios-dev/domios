//! App-side implementations of [`crate::mem::ports`] traits.

use std::collections::HashMap;
use std::sync::Arc;

use crate::cron::store::{self, CronCreateParams, CronStore};
use crate::mem::ports::{
    MemCronCreateParams, MemCronJob, MemEventSink, MemHostContext, MemSchedulerPort,
};
use crate::mem::workers::{self, GraphWorkerInfo, WorkerStatus};
use crate::service::ServiceContext;

pub struct CronSchedulerPort<'a>(pub &'a mut CronStore);

impl MemCronJob {
    fn from_cron(job: &store::CronJob) -> Self {
        Self {
            id: job.id.clone(),
            name: job.name.clone(),
            agent: job.agent.clone(),
            schedule: job.schedule.clone(),
            enabled: job.enabled,
            oneshot: job.oneshot,
            next_run: job.next_run,
            jitter_max_ms: job.jitter_max_ms,
            tz: job.tz.clone(),
            protected: job.protected,
        }
    }
}

impl MemSchedulerPort for CronSchedulerPort<'_> {
    fn list_jobs(&self) -> Vec<MemCronJob> {
        self.0.list().iter().map(MemCronJob::from_cron).collect()
    }

    fn find_job_by_name(&self, name: &str) -> Option<MemCronJob> {
        self.0
            .list()
            .iter()
            .find(|j| j.name == name)
            .map(MemCronJob::from_cron)
    }

    fn update_job(
        &mut self,
        id: &str,
        update: &mut dyn FnMut(&mut MemCronJob),
    ) -> Result<(), String> {
        let job = self
            .0
            .get_mut(id)
            .ok_or_else(|| format!("cron job not found: {id}"))?;
        let mut view = MemCronJob::from_cron(job);
        update(&mut view);
        job.agent = view.agent;
        job.schedule = view.schedule;
        job.jitter_max_ms = view.jitter_max_ms;
        job.next_run = view.next_run;
        job.enabled = view.enabled;
        job.oneshot = view.oneshot;
        job.protected = view.protected;
        job.tz = view.tz;
        Ok(())
    }

    fn set_job_protected(&mut self, id: &str, protected: bool) -> Result<(), String> {
        let job = self
            .0
            .get_mut(id)
            .ok_or_else(|| format!("cron job not found: {id}"))?;
        job.protected = protected;
        Ok(())
    }

    fn create_job(&mut self, params: MemCronCreateParams) -> Result<(), String> {
        self.0
            .create(CronCreateParams {
                name: params.name,
                workspace_id: params.workspace_id,
                schedule: params.schedule,
                task: params.task,
                description: params.description,
                agent: params.agent,
                model: params.model,
                cwd: params.cwd,
                role: params.role,
                creator_pane_id: params.creator_pane_id,
                enabled: params.enabled,
                max_history: params.max_history,
                oneshot: params.oneshot,
                jitter_max_ms: params.jitter_max_ms,
                tz: params.tz,
                timeout_seconds: params.timeout_seconds,
            })
            .map(|_| ())
    }

    fn delete_job(&mut self, id: &str) -> Result<(), String> {
        self.0.delete(id).map(|_| ())
    }

    fn save(&mut self) -> Result<(), String> {
        self.0.save()
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

struct AppMemEventSink(tauri::AppHandle);

impl MemEventSink for AppMemEventSink {
    fn emit_graph_worker_spawned(&self, info: &GraphWorkerInfo) {
        crate::api::events::emit_everywhere(&self.0, "graph-worker-spawned", info);
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
            use crate::service::spawn::{
                AgentSpawnSpec, PaneCommand, PaneVisibility, SpawnPaneSpec,
            };
            let spec = SpawnPaneSpec {
                visibility: PaneVisibility::Hidden,
                title: Some(format!("mem:{}", intent_title)),
                description: None,
                command: PaneCommand::Agent(AgentSpawnSpec {
                    provider: agent_for_spawn,
                    model: if model_for_spawn.is_empty() {
                        None
                    } else {
                        Some(model_for_spawn)
                    },
                    task: Some(task),
                    worker_id: Some(worker_id_blocking.clone()),
                    auto_approve: true,
                    spawned_by: Some(spawned_by_for_spawn),
                    ..Default::default()
                }),
                ..Default::default()
            };
            svc_blocking.spawn_pane(spec)
        })
        .await;

        let handle = match spawn_result {
            Ok(Ok(h)) => h,
            Ok(Err(e)) => {
                log::error!(
                    "[worker] {}/{} spawn_pane failed: {}",
                    intent_for_spawn,
                    worker_id_task,
                    e
                );
                let _ = svc_task
                    .workers
                    .update_status(&worker_id_task, WorkerStatus::Error);
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
                let _ = svc_task
                    .workers
                    .update_status(&worker_id_task, WorkerStatus::Error);
                drop(permit);
                return;
            }
        };

        svc_task
            .workers
            .set_pane_id(&worker_id_task, handle.pane_id.clone());
        log::info!(
            "[worker] headless {} pane={} pty={}",
            worker_id_task,
            handle.pane_id,
            handle.pty_id.as_deref().unwrap_or("?")
        );

        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30 * 60);
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            let Some(current) = svc_task.workers.get(&worker_id_task) else {
                break;
            };
            if current.status.is_terminal() {
                break;
            }
            if tokio::time::Instant::now() > deadline {
                log::warn!(
                    "[worker] {}/{} exceeded 30min watchdog, forcing Orphaned + closing pane",
                    intent_for_spawn,
                    worker_id_task
                );
                let _ = svc_task
                    .workers
                    .update_status(&worker_id_task, WorkerStatus::Orphaned);
                let _ = svc_task.close_pane(&handle.pane_id);
                break;
            }
        }
        drop(permit);
    });

    Ok(worker_id)
}
