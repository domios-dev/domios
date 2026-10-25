use codeterm_service_core::service::ServiceState;
use std::sync::Arc;

pub(crate) fn initialize(service: &ServiceState) {
    let _step = super::StartupStep::logged("background service initialization");
    {
        let _step = super::StartupStep::logged("background: health scheduler");
        let store = service.services_store.clone();
        let activity = codeterm_service_core::service::activity::ActivityWriter::new(store.conn());
        codeterm_service_core::service::health::scheduler::start(
            store,
            activity,
            service.ui.clone(),
        );
    }
    {
        let _step = super::StartupStep::logged("background: librarian spawn");
        let supervisor = service
            .ext::<Arc<codeterm_service_core::mem::supervisor::Supervisor>>()
            .clone();
        codeterm_service_core::async_runtime::spawn(async move {
            loop {
                match supervisor
                    .run(codeterm_service_core::mem::supervisor::WorkerKind::LibrarianOnce)
                    .await
                {
                    Ok(report) => log::info!("[librarian] {}", report),
                    Err(e) => log::warn!("[librarian] {}", e),
                }
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            }
        });
    }

    {
        let _step = super::StartupStep::logged("background: graph layout spawn");
        let mem = service.mem.clone();
        codeterm_service_core::async_runtime::spawn(async move {
            let mem_for_build = mem.clone();
            let _ = codeterm_service_core::async_runtime::spawn_blocking(move || {
                let _step = super::StartupStep::logged("background: graph layout build");
                if mem_for_build.layout_needs_rebuild() {
                    match mem_for_build.run_full_layout() {
                        Ok(v) => log::info!("[graph_layout] startup build v{v}"),
                        Err(e) => log::warn!("[graph_layout] startup build: {e}"),
                    }
                }
            })
            .await;
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                mem.layout_flush_if_due();
            }
        });
    }
    let blueprint_service = service.clone();
    let extraction_service = service.clone();
    spawn_memory_tasks(
        move || codeterm_service_core::mem::blueprint_enrichment::install(&blueprint_service),
        move || {
            if codeterm_service_core::config::memory_config().auto_extract {
                let queued = codeterm_service_core::mem_host::resume_extract_pending(
                    extraction_service,
                    usize::MAX,
                );
                if queued > 0 {
                    log::info!("[mem] startup: resuming extraction for {queued} pending item(s)");
                }
            }
        },
    );

    {
        let _step = super::StartupStep::logged("background: log store");
        let log_store =
            codeterm_service_core::logs::LogStore::open(&codeterm_service_core::paths::data_dir())
                .expect("app_logs.db init");
        log_store.attach_ui(service.ui.clone());
        codeterm_service_core::logs::set_global(log_store.clone());
        log_store.start_flush_loop();
    }

    {
        let _step = super::StartupStep::logged("background: usage poller");
        let poller = codeterm_service_core::usage::UsagePoller::new(
            (**service).clone(),
            service.usage_store.clone(),
            service
                .ext::<codeterm_service_core::usage::TabActivity>()
                .clone(),
        );
        *service
            .ext::<codeterm_service_core::usage::poller::UsagePollerSlot>()
            .0
            .lock() = Some(poller.clone());
        poller.spawn();
    }
}

pub(crate) fn spawn_memory_tasks(
    blueprints: impl FnOnce() + Send + 'static,
    extraction: impl FnOnce() + Send + 'static,
) {
    spawn_memory_task(
        "background: blueprint enrichment spawn",
        "background: blueprint enrichment",
        blueprints,
    );
    spawn_memory_task(
        "background: pending extraction spawn",
        "background: pending extraction",
        extraction,
    );
}

fn spawn_memory_task(
    spawn_name: &'static str,
    name: &'static str,
    work: impl FnOnce() + Send + 'static,
) {
    let _step = super::StartupStep::logged(spawn_name);
    codeterm_service_core::async_runtime::spawn_blocking(move || {
        let _step = super::StartupStep::logged(name);
        work();
    });
}
