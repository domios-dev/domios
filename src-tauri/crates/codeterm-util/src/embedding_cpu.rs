use std::sync::atomic::{AtomicBool, Ordering};

pub const DEFAULT_THREADS: &str = "2";

static DEFAULT_INJECTED: AtomicBool = AtomicBool::new(false);

pub fn record_default_injection() {
    DEFAULT_INJECTED.store(true, Ordering::Release);
}

pub fn default_was_injected() -> bool {
    DEFAULT_INJECTED.load(Ordering::Acquire)
}

pub fn strip_injected_default(command: &mut std::process::Command) {
    strip_inherited_default(
        command,
        default_was_injected(),
        std::env::var_os("CANDLE_NUM_THREADS").as_deref(),
    );
}

fn strip_inherited_default(
    command: &mut std::process::Command,
    injected: bool,
    inherited: Option<&std::ffi::OsStr>,
) {
    // A re-executed daemon must establish its own default-injection provenance.
    if injected
        && inherited == Some(std::ffi::OsStr::new(DEFAULT_THREADS))
        && !command.get_envs().any(|(key, _)| {
            key.to_str()
                .is_some_and(|key| key.eq_ignore_ascii_case("CANDLE_NUM_THREADS"))
        })
    {
        command.env_remove("CANDLE_NUM_THREADS");
    }
}

#[cfg(test)]
mod tests;
