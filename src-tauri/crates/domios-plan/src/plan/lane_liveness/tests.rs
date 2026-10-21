use super::*;

fn base(now: u64) -> LaneInputs {
    LaneInputs {
        has_assigned_tab: true,
        tab_present_local: true,
        has_peer: false,
        peer_online: None,
        tracker_active: Some(true),
        last_activity_ms: now,
        now_ms: now,
    }
}

#[test]
fn no_tab_short_circuits() {
    let mut i = base(1000);
    i.has_assigned_tab = false;
    assert_eq!(
        derive_lane_liveness(&i, &LaneThresholds::default()),
        LaneLiveness::NoTab
    );
    // The watchdog invents no state for tab-less tasks.
    let v = verdict_for(LaneLiveness::NoTab, &i);
    assert!(v.status_label.is_none() && !v.notify_orchestrator && v.stride.is_none());
}

#[test]
fn working_when_local_and_active() {
    let i = base(1000);
    assert_eq!(
        derive_lane_liveness(&i, &LaneThresholds::default()),
        LaneLiveness::Working
    );
}

#[test]
fn idle_stalled_after_threshold() {
    let now = 100 * 60 * 1000;
    let mut i = base(now);
    i.tracker_active = Some(false);
    i.last_activity_ms = now - (DEFAULT_T_STALL_MS + 1);
    assert_eq!(
        derive_lane_liveness(&i, &LaneThresholds::default()),
        LaneLiveness::IdleStalled
    );
    let v = verdict_for(LaneLiveness::IdleStalled, &i);
    assert_eq!(v.status_label, Some(STALLED_LABEL));
    assert!(v.notify_orchestrator && v.stride.is_some());
}

#[test]
fn idle_but_fresh_is_working() {
    let now = 100 * 60 * 1000;
    let mut i = base(now);
    i.tracker_active = Some(false);
    i.last_activity_ms = now - 1000;
    assert_eq!(
        derive_lane_liveness(&i, &LaneThresholds::default()),
        LaneLiveness::Working
    );
}

#[test]
fn report_overdue_is_amber_not_gone() {
    let now = 100 * 60 * 1000;
    let mut i = base(now);
    i.tracker_active = Some(true);
    i.last_activity_ms = now - (DEFAULT_T_OVERDUE_MS + 1);
    assert_eq!(
        derive_lane_liveness(&i, &LaneThresholds::default()),
        LaneLiveness::ReportOverdue
    );
    let v = verdict_for(LaneLiveness::ReportOverdue, &i);
    assert_eq!(v.status_label, Some(REPORT_OVERDUE_LABEL));
}

#[test]
fn tab_gone_only_without_peer() {
    let mut i = base(1000);
    i.tab_present_local = false;
    i.has_peer = false;
    assert_eq!(
        derive_lane_liveness(&i, &LaneThresholds::default()),
        LaneLiveness::TabGone
    );
    let v = verdict_for(LaneLiveness::TabGone, &i);
    assert_eq!(v.status_label, Some(NEEDS_REASSIGN_LABEL));
    assert!(v.notify_orchestrator);
}

#[test]
fn remote_tab_absent_locally_stays_working() {
    let mut i = base(1000);
    i.tab_present_local = false;
    i.has_peer = true;
    i.peer_online = Some(true);
    assert_eq!(
        derive_lane_liveness(&i, &LaneThresholds::default()),
        LaneLiveness::Working
    );
}

#[test]
fn peer_offline_parks_verdict_and_never_reassigns() {
    let mut i = base(1000);
    i.tab_present_local = false;
    i.has_peer = true;
    i.peer_online = Some(false);
    assert_eq!(
        derive_lane_liveness(&i, &LaneThresholds::default()),
        LaneLiveness::PeerOffline
    );
    let v = verdict_for(LaneLiveness::PeerOffline, &i);
    assert_eq!(v.status_label, Some(PEER_OFFLINE_LABEL));
    // A sleeping laptop must not trigger reassignment.
    assert!(!v.notify_orchestrator);
}

#[test]
fn watchdog_never_carries_status_transition() {
    // LaneVerdict has no status field at all — flags/records/notify only.
    for liveness in [
        LaneLiveness::Working,
        LaneLiveness::IdleStalled,
        LaneLiveness::ReportOverdue,
        LaneLiveness::TabGone,
        LaneLiveness::PeerOffline,
    ] {
        let v = verdict_for(liveness, &base(1000));
        // Notifying verdicts always carry an evidence stride.
        assert_eq!(
            v.notify_orchestrator,
            v.stride.is_some() && v.notify_orchestrator
        );
    }
}
