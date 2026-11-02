use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

const PROVIDER_WRITES_DEADLINE: Duration = Duration::from_secs(60);
const CLI_WRITER: &str = "bundled CLI installation";
const RULES_WRITER: &str = "provider rules and General instruction files";

pub(crate) struct ProviderWritesSettled(());

pub(crate) fn complete(
    cli: Option<std::thread::JoinHandle<()>>,
    integrations: impl IntoIterator<Item = (String, impl FnOnce(Instant) -> bool)>,
    install_rules: impl FnOnce() + Send + 'static,
) -> ProviderWritesSettled {
    complete_until(
        cli,
        integrations,
        install_rules,
        Instant::now() + PROVIDER_WRITES_DEADLINE,
        |writer, failure| {
            log::warn!(
                "[startup-provider-writes] writer={writer} failure={failure}; continuing startup without waiting for this writer"
            );
        },
    )
}

fn complete_until(
    cli: Option<std::thread::JoinHandle<()>>,
    integrations: impl IntoIterator<Item = (String, impl FnOnce(Instant) -> bool)>,
    install_rules: impl FnOnce() + Send + 'static,
    deadline: Instant,
    mut record_failure: impl FnMut(&str, &str),
) -> ProviderWritesSettled {
    if let Some(cli) = cli {
        wait_for_writer(
            CLI_WRITER,
            spawn_writer(move || cli.join().map_err(|_| "writer panicked".into())),
            deadline,
            &mut record_failure,
        );
    }
    for (name, integration) in integrations {
        if !integration(deadline) {
            record_failure(
                &name,
                "total startup writer deadline elapsed; writer unfinished",
            );
        }
    }
    wait_for_writer(
        RULES_WRITER,
        spawn_writer(move || {
            install_rules();
            Ok(())
        }),
        deadline,
        &mut record_failure,
    );
    ProviderWritesSettled(())
}

fn spawn_writer(
    work: impl FnOnce() -> Result<(), String> + Send + 'static,
) -> Result<Receiver<Result<(), String>>, String> {
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("startup-provider-writer".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
                .unwrap_or_else(|_| Err("writer panicked".into()));
            let _ = sender.send(result);
        })
        .map_err(|error| format!("writer thread failed: {error}"))?;
    Ok(receiver)
}

fn wait_for_writer(
    name: &str,
    receiver: Result<Receiver<Result<(), String>>, String>,
    deadline: Instant,
    record_failure: &mut impl FnMut(&str, &str),
) {
    let result = receiver.and_then(|receiver| {
        receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|error| match error {
                RecvTimeoutError::Timeout => {
                    "total startup writer deadline elapsed; writer unfinished".to_string()
                }
                RecvTimeoutError::Disconnected => "writer completion unavailable".to_string(),
            })?
    });
    if let Err(failure) = result {
        record_failure(name, &failure);
    }
}

#[cfg(test)]
mod tests;
