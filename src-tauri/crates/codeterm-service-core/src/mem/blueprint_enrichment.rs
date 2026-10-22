use crate::service::ServiceContext;
use codeterm_mem::blueprint_library::{BlueprintEnricher, EnrichmentJob};
use std::sync::{Arc, Weak};

pub const INTENT_PREFIX: &str = "blueprint-enrich:";

struct WorkerEnricher {
    svc: Weak<ServiceContext>,
}

impl BlueprintEnricher for WorkerEnricher {
    fn request(&self, job: EnrichmentJob) -> Result<(), String> {
        let svc = self.svc.upgrade().ok_or("service is shutting down")?;
        let configuration = svc.configuration();
        let resolved = codeterm_mem::worker_agent::resolve_worker(
            None,
            "",
            "",
            &configuration.general_agent.agent_provider,
            &configuration.memory.worker_model,
            &crate::service::spawn::CatalogBackgroundModelPort,
        );
        let model = if resolved.model.is_empty() {
            format!("{} default", resolved.agent)
        } else {
            resolved.model.clone()
        };
        let intent = format!(
            "{INTENT_PREFIX}{}:{}",
            job.item_id,
            job.document_hash.get(..12).unwrap_or(&job.document_hash)
        );
        let worker_id = crate::mem_host::spawn_headless(
            &svc,
            intent,
            job.task,
            "system:blueprint-library".into(),
            resolved.agent,
            resolved.model,
        )?;
        let EnrichmentJob {
            item_id,
            document_hash,
            ..
        } = job;
        crate::async_runtime::spawn(async move {
            svc.workers.wait_terminal(&worker_id).await;
            let Some(report) = svc.workers.get(&worker_id).and_then(|worker| worker.report) else {
                log::info!("[blueprint-memory] enrichment worker {worker_id} ended without a report; {item_id} stays pending");
                return;
            };
            let mem = svc.mem.clone();
            let outcome = tokio::task::spawn_blocking(move || {
                mem.complete_blueprint_enrichment(&item_id, &document_hash, &model, &report)
            })
            .await;
            match outcome {
                Ok(Ok(applied)) => {
                    log::debug!("[blueprint-memory] enrichment {worker_id} applied={applied}")
                }
                Ok(Err(error)) => {
                    log::info!("[blueprint-memory] enrichment {worker_id} refused: {error}")
                }
                Err(error) => log::warn!("[blueprint-memory] enrichment {worker_id}: {error}"),
            }
        });
        Ok(())
    }
}

pub fn install(svc: &Arc<ServiceContext>) {
    svc.mem
        .set_blueprint_enricher(Some(Arc::new(WorkerEnricher {
            svc: Arc::downgrade(svc),
        })));
    if let Err(error) = svc.mem.queue_blueprint_backfill() {
        log::warn!("[blueprint-memory] backfill not queued: {error}");
    }
}
