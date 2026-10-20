use super::*;

#[test]
fn third_trailing_stagnant_bounce_returns_strategy_warning_atomically() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "stagnant", 2);
    dispatch(&store, &plan_id, &task_id, "tab_impl", 3).unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();
    store
        .with_plan(&plan_id, 4, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            for at in [2, 3] {
                task.review_evidence.push(ReviewEvidence {
                    id: format!("rev_{at}"),
                    from: AgentRef::from_tab("reviewer"),
                    verdict: ReviewVerdict::Bounce,
                    note: "same approach".into(),
                    at,
                    changed_executable_surfaces: false,
                    distinct_oracle: false,
                });
                task.review_round += 1;
            }
            Ok(())
        })
        .unwrap();

    let result = store
        .bounce_review(
            &plan_id,
            &task_id,
            "same approach still fails",
            false,
            false,
            AgentRef::from_tab("reviewer"),
            5,
        )
        .unwrap();
    let warning = result.strategy_warning.unwrap();
    assert_eq!(warning.consecutive_bounces, 3);
    assert_eq!(warning.review_round, 3);
    assert!(warning.message.contains("codeterm docs BOUNCE"));
    assert!(
        warning.decisions.is_empty(),
        "a task with no recorded decisions still warns, carrying an empty set rather than being suppressed"
    );
}

#[test]
fn strategy_warning_carries_decisions_selected_by_stride_kind() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "stagnant-with-decisions", 2);
    dispatch(&store, &plan_id, &task_id, "tab_impl", 3).unwrap();
    set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();
    store
        .with_plan(&plan_id, 4, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            for at in [2, 3] {
                task.review_evidence.push(ReviewEvidence {
                    id: format!("rev_{at}"),
                    from: AgentRef::from_tab("reviewer"),
                    verdict: ReviewVerdict::Bounce,
                    note: "same approach".into(),
                    at,
                    changed_executable_surfaces: false,
                    distinct_oracle: false,
                });
                task.review_round += 1;
            }
            // Same text on both strides, different kind: proves selection keys
            // off `kind`, never off scanning rendered text for content.
            task.milestones.push(Milestone {
                id: "ms_note".into(),
                from: AgentRef::from_tab("orch"),
                kind: "note".into(),
                text: "use approach A for the retry".into(),
                ts: 3,
                links: vec![],
                loop_id: None,
                iteration: None,
                canvas: Default::default(),
                attachments: vec![],
            });
            task.milestones.push(Milestone {
                id: "ms_decision".into(),
                from: AgentRef::from_tab("orch"),
                kind: DECISION_STRIDE_KIND.into(),
                text: "use approach A for the retry".into(),
                ts: 4,
                links: vec![],
                loop_id: None,
                iteration: None,
                canvas: Default::default(),
                attachments: vec![],
            });
            Ok(())
        })
        .unwrap();

    let result = store
        .bounce_review(
            &plan_id,
            &task_id,
            "same approach still fails",
            false,
            false,
            AgentRef::from_tab("reviewer"),
            5,
        )
        .unwrap();
    let warning = result.strategy_warning.unwrap();
    assert_eq!(warning.consecutive_bounces, 3);
    assert_eq!(
        warning.decisions,
        vec![DecisionStride {
            id: "ms_decision".into(),
            text: "use approach A for the retry".into(),
            ts: 4,
        }],
        "only the decision-kind stride surfaces in the payload, never the note stride with matching text"
    );
}

#[test]
fn strategy_warning_resets_on_accept_changed_surface_or_distinct_oracle() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "resets", 2);
    let base = ReviewEvidence {
        id: "rev".into(),
        from: AgentRef::from_tab("reviewer"),
        verdict: ReviewVerdict::Bounce,
        note: "finding".into(),
        at: 1,
        changed_executable_surfaces: false,
        distinct_oracle: false,
    };
    let mut task = store.show_task(&task_id).unwrap().task;
    task.review_round = 4;
    task.review_evidence = vec![base.clone(), base.clone(), base.clone()];
    assert!(strategy_warning(&task).is_some());

    for reset in [
        ReviewEvidence {
            verdict: ReviewVerdict::Accept,
            ..base.clone()
        },
        ReviewEvidence {
            changed_executable_surfaces: true,
            ..base.clone()
        },
        ReviewEvidence {
            distinct_oracle: true,
            ..base.clone()
        },
    ] {
        task.review_evidence.push(reset);
        assert_eq!(strategy_warning(&task), None);
        task.review_evidence.pop();
    }
}
