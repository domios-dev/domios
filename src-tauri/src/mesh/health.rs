//! Online/offline decision for paired peers.
//!
//! Single writer: the background mesh health loop (multi-address `try_peer`
//! ping). The old single-address `ping_scheduler` was deleted — two loops
//! disagreeing about `online` made the flag flap whenever the best-tier
//! address was dead but another one worked (#127).

const MISS_THRESHOLD: u8 = 2;

pub struct PingOutcome {
    pub online: bool,
    pub misses: u8,
}

/// Apply one multi-address ping round. A reachable peer comes back online
/// immediately; an online peer needs `MISS_THRESHOLD` consecutive failed
/// rounds before flipping offline, so one transient glitch doesn't drop it.
pub fn apply_ping_result(prev_online: bool, prev_misses: u8, ping_ok: bool) -> PingOutcome {
    if ping_ok {
        return PingOutcome {
            online: true,
            misses: 0,
        };
    }
    let misses = prev_misses.saturating_add(1);
    PingOutcome {
        online: prev_online && misses < MISS_THRESHOLD,
        misses,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_miss_does_not_flip_online_peer_offline() {
        let out = apply_ping_result(true, 0, false);
        assert!(out.online);
        assert_eq!(out.misses, 1);
    }

    #[test]
    fn two_misses_flips_online_peer_offline() {
        let out = apply_ping_result(true, 1, false);
        assert!(!out.online);
        assert_eq!(out.misses, 2);
    }

    #[test]
    fn offline_peer_stays_offline_on_miss() {
        let out = apply_ping_result(false, 5, false);
        assert!(!out.online);
        assert_eq!(out.misses, 6);
    }

    #[test]
    fn successful_ping_resets_misses_and_revives() {
        let out = apply_ping_result(false, 200, true);
        assert!(out.online);
        assert_eq!(out.misses, 0);
    }

    #[test]
    fn miss_counter_saturates() {
        let out = apply_ping_result(false, u8::MAX, false);
        assert_eq!(out.misses, u8::MAX);
        assert!(!out.online);
    }
}
