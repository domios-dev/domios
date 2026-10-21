use super::*;
use crate::plan::Goal;

#[test]
fn curator_synthesis_cadence_counts_real_rounds_and_new_group_closes() {
    for started in 0..18 {
        assert_eq!(due(started, None, None), (started + 1) % 6 == 0);
    }
    assert!(due(1, Some(100), None));
    assert!(!due(1, Some(100), Some(100)));
    assert!(due(1, Some(101), Some(100)));
    assert!(!due(1, Some(99), Some(100)));
}

#[test]
fn curator_synthesis_history_keeps_newest_lines_within_digest_cap() {
    let mut plan = Plan::new("goal fixture".into(), "/tmp".into(), Default::default());
    plan.goal = Some(Goal {
        vector: "opaque correctness vector".into(),
        vision: "opaque goal".into(),
        ..Default::default()
    });
    for number in 1..=120 {
        super::super::append_round(
            &mut plan,
            CuratorRound {
                number,
                ended_at: number * 1000,
                digest_hash: "opaque".into(),
                outcome: "done".into(),
                move_line: format!("move-{number} [task_{number}]"),
                retro: vec![format!("+ retro-{number} [task_{number}]")],
                learn: vec![format!(
                    "when episode-{number} -> act [task_{number}] (confidence low)"
                )],
                ..Default::default()
            },
        );
    }
    let header = context_header(&plan, "synthesis: yes");
    let digest = super::super::digest::build(&header, &Default::default());
    assert!(digest.body.len() <= MAX_DIGEST_BYTES);
    assert!(digest.body.contains("opaque correctness vector"));
    assert!(digest.body.contains("open acceptance criteria"));
    assert!(digest.body.find("move-120 ").unwrap() < digest.body.find("move-119 ").unwrap());
    assert!(!digest.body.contains("move-1 "));
    assert!(digest.body.contains("RETRO | + retro-120"));
    assert!(digest.body.contains("LEARN | when episode-120"));
    assert_eq!(real_rounds_started(&plan), 120);
    super::super::append_round(
        &mut plan,
        CuratorRound {
            outcome: "quiet".into(),
            ..Default::default()
        },
    );
    assert_eq!(real_rounds_started(&plan), 120);
}

#[test]
fn curator_ready_is_advisory_and_only_accepted_in_synthesis_rounds() {
    let text =
        "NEXT: 45m | reason\nMOVE: inspect [task_1]\nREADY: yes evidence [task_1]\nASKED: none";
    assert!(super::super::note::parse(text, 2700).is_err());
    let note = super::super::note::parse_for_round(text, 2700, true).unwrap();
    assert_eq!(note.ready.as_deref(), Some("yes evidence [task_1]"));
    assert!(super::super::note::parse_for_round(
        &text.replace("yes evidence [task_1]", "maybe [task_1]"),
        2700,
        true
    )
    .is_err());
    assert!(super::super::note::parse_for_round(
        &text.replace("yes evidence [task_1]", "no evidence"),
        2700,
        true
    )
    .is_err());
    assert!(super::super::note::parse_for_round(
        &text.replace("READY: yes evidence [task_1]\n", ""),
        2700,
        true
    )
    .is_ok());
}
