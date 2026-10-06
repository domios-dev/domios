use super::*;
use crate::service::supporter::{activity::DecisionEvidence, SupporterRuntime, VerdictKind};

#[test]
fn supporter_feed_tab_state_groups_named_questions_without_option_text() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = SupporterRuntime::new(dir.path().into());
    let title = "supplied-tab-title";
    let private_option = "screen-derived-option-fixture";
    let context = ActivityContext::TabState {
        title: title.into(),
        declared_safe_choice: Some(private_option.into()),
        decision_point: "launch_ready".into(),
    };
    let response = codeterm_types::decision::DecisionResponse {
        answers: DecisionAnswers::from([
            (
                "label".into(),
                DecisionAnswer::Choice {
                    choice: "ready_idle".into(),
                    probabilities: Default::default(),
                    confidence: Some(0.9),
                },
            ),
            ("prompt_visible".into(), DecisionAnswer::Noul { p: 0.75 }),
            (
                "safe_choice".into(),
                DecisionAnswer::Choice {
                    choice: private_option.into(),
                    probabilities: Default::default(),
                    confidence: Some(0.9),
                },
            ),
        ]),
        usage: None,
    };
    runtime.response(
        VerdictKind::TabState,
        None,
        &response,
        DecisionEvidence {
            decision_point: Some(context.decision_point()),
            model_id: None,
            criteria_version: "fixture",
            state_hash: "fixture",
            question_set_hash: "fixture",
            asked: Some(context.answers(&response.answers)),
        },
    );
    let feed = runtime.feed(10, None).unwrap();
    assert_eq!(feed.len(), 1);
    assert_eq!(feed[0]["decision_point"], "launch_ready");
    let asked = feed[0]["asked"].as_array().unwrap();
    assert_eq!(asked.len(), 3);
    assert_eq!(
        asked
            .iter()
            .map(|pair| pair["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["label", "prompt_visible", "safe_choice"]
    );
    for pair in asked {
        assert!(pair["question"].as_str().unwrap().contains(title));
        assert!(!pair["answer"].as_str().unwrap().is_empty());
    }
    assert!(asked[1]["answer"].as_str().unwrap().contains("75"));
    assert!(!feed[0].to_string().contains(private_option));
}

#[test]
fn supporter_feed_unknown_bucket_and_option_values_do_not_pass_through() {
    let private = "unknown-raw-choice-fixture";
    let answers = DecisionAnswers::from([(
        "label".into(),
        DecisionAnswer::Choice {
            choice: private.into(),
            probabilities: Default::default(),
            confidence: Some(0.9),
        },
    )]);
    let tab = ActivityContext::TabState {
        title: "fixture".into(),
        declared_safe_choice: None,
        decision_point: "startup_deadline".into(),
    }
    .answers(&answers);
    let sentinel = ActivityContext::Sentinel {
        job: "fixture".into(),
        buckets: vec!["allowed".into()],
    }
    .answers(&answers);
    for pair in tab.iter().chain(sentinel.iter()) {
        assert!(!pair.answer.contains(private));
    }
}

#[test]
fn supporter_feed_safe_choice_distinguishes_declared_other_and_abstention() {
    let declared = "supplied-safe-option";
    let other = "supplied-other-option";
    let context = ActivityContext::TabState {
        title: "fixture".into(),
        declared_safe_choice: Some(declared.into()),
        decision_point: "user_delivery_readiness".into(),
    };
    let outputs: Vec<_> = [declared, other, "__abstain__"]
        .iter()
        .map(|choice| {
            let answers = DecisionAnswers::from([(
                "safe_choice".into(),
                DecisionAnswer::Choice {
                    choice: (*choice).into(),
                    probabilities: Default::default(),
                    confidence: Some(0.9),
                },
            )]);
            context.answers(&answers)[0].answer.clone()
        })
        .collect();
    assert_eq!(
        outputs
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
    for output in outputs {
        assert!(!output.contains(declared));
        assert!(!output.contains(other));
        assert!(!output.contains("__abstain__"));
    }
}
