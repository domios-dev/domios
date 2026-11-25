use super::*;
use crate::service::agent_runtime::process_boundary::SelectedBackgroundChild;
use codeterm_plugin::agent_runtime::validate_background_processes;

fn selected(pid: u32, born: u128, command: &str) -> SelectedBackgroundProcess {
    SelectedBackgroundProcess {
        pid,
        born,
        parent_pid: None,
        command: command.into(),
        cwd: "/work".into(),
        children: Vec::new(),
    }
}

#[test]
fn idle_surfaces_os_and_bridge_work_active_turn_keeps_tracked_only_and_clears_on_exit() {
    let mut tracked = HashMap::new();
    let mut bg_cmd = selected(5000, 100, "cargo nextest run -p codeterm-service-core");
    bg_cmd.children = vec![SelectedBackgroundChild {
        pid: 5002,
        born: 101,
        command: "sccache".into(),
    }];
    let fg_turn2 = selected(5001, 200, "git status");

    // Foreground command during an active turn is never surfaced prematurely.
    let (during_turn1, kept) =
        reconcile_os_entries("root", true, &[bg_cmd.clone()], &[], &mut tracked);
    assert!(during_turn1.is_empty());
    assert!(kept.is_empty());
    assert!(tracked.is_empty());

    // Once the turn settles to idle, surviving OS subprocesses and bridge terminals surface.
    let bridge = vec![(
        "term-1".into(),
        "node watch.mjs".into(),
        "/work".into(),
        6000u32,
    )];
    let (after_turn1, kept) =
        reconcile_os_entries("root", false, &[bg_cmd.clone()], &bridge, &mut tracked);
    assert_eq!(after_turn1.len(), 2);
    assert_eq!(after_turn1[0].thread_id, "root");
    assert_eq!(after_turn1[0].process.process_id, "os:5000:100");
    assert_eq!(after_turn1[0].process.command, bg_cmd.command);
    assert_eq!(after_turn1[0].process.os_pid, Some(5000));
    assert_eq!(
        after_turn1[0].children,
        Some(vec![BackgroundProcessChild {
            command: "sccache".into(),
            os_pid: Some(5002),
        }])
    );
    assert_eq!(after_turn1[1].process.process_id, "bridge:term-1");
    assert_eq!(after_turn1[1].process.command, "node watch.mjs");
    assert!(kept.contains(&(5000, 100)));
    assert!(kept.contains(&(5002, 101)));
    let raw_processes: Vec<_> = after_turn1
        .iter()
        .map(|entry| entry.process.clone())
        .collect();
    validate_background_processes(&raw_processes).expect("entries satisfy protocol schema");

    // A new active turn preserves the previously tracked background task while hiding new foreground commands.
    let (during_turn2, kept) = reconcile_os_entries(
        "root",
        true,
        &[bg_cmd.clone(), fg_turn2.clone()],
        &[],
        &mut tracked,
    );
    assert_eq!(during_turn2.len(), 1);
    assert_eq!(during_turn2[0].process.process_id, "os:5000:100");
    assert_eq!(kept, HashSet::from([(5000, 100), (5002, 101)]));

    // If the root exits mid-turn while a tracked helper lives on, the surviving helper stays visible.
    let surviving_helper = selected(5002, 101, "sccache");
    let (helper_only, kept) = reconcile_os_entries(
        "root",
        true,
        &[surviving_helper, fg_turn2.clone()],
        &[],
        &mut tracked,
    );
    assert_eq!(helper_only.len(), 1);
    assert_eq!(helper_only[0].process.process_id, "os:5002:101");
    assert_eq!(kept, HashSet::from([(5002, 101)]));

    // When the background command exits mid-turn, it disappears immediately.
    let (after_exit_mid_turn, kept) =
        reconcile_os_entries("root", true, &[fg_turn2], &[], &mut tracked);
    assert!(after_exit_mid_turn.is_empty());
    assert!(kept.is_empty());
    assert!(tracked.is_empty());

    // Settling idle with no remaining subprocesses keeps the roster empty.
    let (idle_empty, _) = reconcile_os_entries("root", false, &[], &[], &mut tracked);
    assert!(idle_empty.is_empty());
}
