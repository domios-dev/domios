/// The sweep walks provider plugin VMs and the disk, so it runs off the startup-critical path.
pub fn notify(
    service: &codeterm_service_core::service::ServiceState,
) -> std::thread::JoinHandle<()> {
    let service = service.clone();
    std::thread::Builder::new()
        .name("launch-artifact-sweep".into())
        .spawn(move || {
            let started = std::time::Instant::now();
            sweep(&service);
            log::info!(
                "[startup] launch artifact sweep took {:?}",
                started.elapsed()
            );
        })
        .expect("spawn launch artifact sweep thread")
}

fn sweep(service: &codeterm_service_core::service::ServiceContext) {
    codeterm_service_core::service::restore_utility_deadlines(service);
    codeterm_service_core::service::launch_artifacts::sweep_after_restore(service);
}

#[cfg(test)]
mod tests;
