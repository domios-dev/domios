use codeterm_types::config::MemoryConfig;

pub(crate) const WORKER_WATCHDOG_GRACE_SECS: u64 = 120;
const MAX_WORKER_RUN_SECS: u64 = 45 * 60;

pub(crate) fn worker_watchdog_deadline_secs(run_deadline_secs: u64) -> u64 {
    (run_deadline_secs + WORKER_WATCHDOG_GRACE_SECS).min(MAX_WORKER_RUN_SECS)
}

pub(crate) fn worker_deadline_secs(intent: &str, config: &MemoryConfig) -> u64 {
    let configured = if crate::mem::workers::is_dreaming_intent(intent) {
        config.dreaming_timeout_seconds
    } else {
        config.worker_timeout_seconds
    };
    configured.min(MAX_WORKER_RUN_SECS)
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
        let watchdog_deadline = worker_watchdog_deadline_secs(run_deadline);

        assert_eq!(WORKER_WATCHDOG_GRACE_SECS, 120);
        assert!(watchdog_deadline > run_deadline);
    }

    #[test]
    fn memory_workers_have_a_forty_five_minute_ceiling() {
        let config = MemoryConfig {
            worker_timeout_seconds: 7200,
            dreaming_timeout_seconds: 7200,
            ..MemoryConfig::default()
        };
        for intent in ["extract:item", "dreaming:rem"] {
            assert_eq!(worker_deadline_secs(intent, &config), 2700);
            assert_eq!(
                worker_watchdog_deadline_secs(worker_deadline_secs(intent, &config)),
                2700
            );
        }
    }
}
