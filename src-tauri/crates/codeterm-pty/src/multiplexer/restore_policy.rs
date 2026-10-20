#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreDecision {
    Start,
    Freeze,
}

pub fn restore_on_boot(
    is_general_workspace: bool,
    protected: bool,
    last_active_at_ms: Option<u64>,
    now_ms: u64,
    window_ms: u64,
) -> RestoreDecision {
    if protected {
        RestoreDecision::Freeze
    } else if is_general_workspace
        || last_active_at_ms.is_some_and(|last| now_ms.saturating_sub(last) <= window_ms)
    {
        RestoreDecision::Start
    } else {
        RestoreDecision::Freeze
    }
}

#[cfg(test)]
mod tests {
    use super::{restore_on_boot, RestoreDecision::*};

    #[test]
    fn general_starts_without_activity() {
        assert_eq!(restore_on_boot(true, false, None, 100, 10), Start);
        assert_eq!(restore_on_boot(true, false, Some(0), 100, 10), Start);
    }

    #[test]
    fn activity_window_is_inclusive() {
        assert_eq!(restore_on_boot(false, false, Some(90), 100, 10), Start);
        assert_eq!(restore_on_boot(false, false, Some(89), 100, 10), Freeze);
        assert_eq!(restore_on_boot(false, false, None, 100, 10), Freeze);
        assert_eq!(restore_on_boot(false, false, Some(110), 100, 10), Start);
    }

    #[test]
    fn protected_keeps_its_dedicated_startup_owner() {
        assert_eq!(restore_on_boot(true, true, Some(100), 100, 10), Freeze);
        assert_eq!(restore_on_boot(false, true, None, 100, 10), Freeze);
    }
}
