use codeterm_types::config::MemoryConfig;

pub(crate) const WORKER_WATCHDOG_GRACE_SECS: u64 = 120;

pub(crate) fn worker_deadline_secs(intent: &str, config: &MemoryConfig) -> u64 {
    if crate::mem::workers::is_dreaming_intent(intent) {
        config.dreaming_timeout_seconds
    } else {
        config.worker_timeout_seconds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> MemoryConfig {
        MemoryConfig {
            worker_timeout_seconds: 45,
            dreaming_timeout_seconds: 900,
            ..MemoryConfig::default()
        }
    }

    #[test]
    fn worker_deadline_follows_dreaming_timeout_config() {
        assert_eq!(worker_deadline_secs("dreaming:rem", &config()), 900);
    }

    #[test]
    fn worker_deadline_follows_worker_timeout_for_non_dreaming() {
        assert_eq!(worker_deadline_secs("extraction", &config()), 45);
    }

    #[test]
    fn watchdog_backstop_exceeds_run_watcher_deadline() {
        let run_deadline = worker_deadline_secs("dreaming:rem", &config());
        let watchdog_deadline = run_deadline + WORKER_WATCHDOG_GRACE_SECS;

        assert_eq!(WORKER_WATCHDOG_GRACE_SECS, 120);
        assert!(watchdog_deadline > run_deadline);
    }
}
