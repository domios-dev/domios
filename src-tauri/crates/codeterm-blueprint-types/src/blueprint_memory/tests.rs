use super::*;
use crate::blueprint_document::Input;

fn graph(repo: &str, id: u64) -> Document {
    Document::from_definition(&json!({"schema_version":2,"kind":"pipeline","name":"CI watcher","note":"Concrete instance","owner":"team","trigger":{"type":"schedule","every":{"lit":"1m"}},"steps":[{"id":"check","uses":"github","with":{"resource":{"lit":"run"},"repo":{"lit":repo},"id":{"lit":id}},"routes":{"done":"@stop","failing":"@stop","failed":"@stop"}}],"result":{"to":"notify"}})).unwrap()
}
fn entry(id: &str, uses: u64) -> Entry {
    Entry {
        id: id.into(),
        document: abstract_document(graph("org/repo", 42)),
        uses,
        last_used: 100,
        intents: vec!["Watch CI run and report failing jobs".into()],
        successes: 1,
        failures: 0,
        kept: false,
    }
}
fn evidence() -> Evidence {
    Evidence {
        successes: 1,
        recognized: 1,
        runs: 1,
        note: "Watch CI".into(),
        safe: true,
        ..Evidence::default()
    }
}

#[test]
fn qualification_requires_success_recognized_outcomes_and_a_note() {
    assert_eq!(decide(&evidence()).reason, "proven_solution");
    assert_eq!(
        decide(&Evidence {
            successes: 0,
            ..evidence()
        })
        .reason,
        "no_success"
    );
    assert_eq!(
        decide(&Evidence {
            unclassified: 2,
            ..evidence()
        })
        .reason,
        "mostly_unrecognized_or_failed"
    );
    assert_eq!(
        decide(&Evidence {
            failures: 2,
            ..evidence()
        })
        .reason,
        "mostly_unrecognized_or_failed"
    );
    assert_eq!(
        decide(&Evidence {
            note: String::new(),
            ..evidence()
        })
        .reason,
        "missing_note"
    );
    assert_eq!(
        decide(&Evidence {
            note: String::new(),
            name: "tmp-test".into(),
            ..evidence()
        })
        .reason,
        "throwaway"
    );
}

#[test]
fn qualification_rejects_early_deletion_and_error_stop_but_keep_only_bypasses_quality() {
    assert_eq!(
        decide(&Evidence {
            created_at_ms: 1,
            creator_deleted_at_ms: Some(600_000),
            ..evidence()
        })
        .reason,
        "deleted_as_mistake"
    );
    assert!(
        decide(&Evidence {
            creator_deleted_at_ms: Some(600_000),
            ..evidence()
        })
        .save
    );
    assert_eq!(
        decide(&Evidence {
            stopped_before_success: true,
            ..evidence()
        })
        .reason,
        "stopped_before_success"
    );
    assert!(
        decide(&Evidence {
            kept: true,
            safe: true,
            ..Evidence::default()
        })
        .save
    );
    assert!(
        !decide(&Evidence {
            kept: true,
            safe: false,
            ..Evidence::default()
        })
        .save
    );
}

#[test]
fn generalisation_removes_instances_and_keeps_described_typed_required_inputs() {
    let doc = abstract_document(graph("owner/service", 12345));
    let yaml = doc.yaml().unwrap();
    assert!(!yaml.contains("owner/service"));
    assert!(!yaml.contains("12345"));
    assert_eq!(doc.inputs["repo"].default, None);
    assert_eq!(doc.inputs["id"].kind, "integer");
    assert!(doc.inputs.values().all(|input| input.description.is_some()));
    let bound = doc
        .bind(&["repo=another/service".into(), "id=999".into()])
        .unwrap();
    assert_eq!(bound["steps"][0]["with"]["id"]["lit"], 999);
    assert!(crate::blueprint_document::definition_errors(&bound).is_empty());
    assert_eq!(parse_document(&yaml).unwrap().inputs.len(), 2);
    assert_eq!(
        abstract_document(doc.clone()).yaml().unwrap(),
        doc.yaml().unwrap()
    );
}

#[test]
fn safety_drops_query_credentials_and_personal_paths_even_from_defaults() {
    let mut doc = graph("owner/service", 1);
    doc.steps[0] = serde_json::from_value(json!({"id":"private","uses":"http","with":{"url":{"lit":"https://user:pass@example.test/health?token=private-token"}},"routes":{"up":"@stop","down":"@stop","failed":"@stop"}})).unwrap();
    doc.inputs.insert(
        "personal_file".into(),
        Input {
            kind: "string".into(),
            default: Some(json!("C:/Users/owner/private.txt")),
            description: Some("private".into()),
            choices: None,
            ..Default::default()
        },
    );
    let general = abstract_document(doc);
    let yaml = general.yaml().unwrap();
    assert!(!yaml.contains("private-token"));
    assert!(!yaml.contains("user:pass"));
    assert!(!yaml.contains("C:/Users"));
    assert!(safe_template(&general, "Check HTTP health"));
    assert!(!safe_template(&general, "token=private-token"));
    assert!(!safe_template(&general, "Read /home/owner/private.txt"));
}

#[test]
fn structural_dedupe_ignores_instance_values_and_names_but_keeps_routes() {
    let library = vec![entry("saved", 4)];
    let mut candidate = abstract_document(graph("other/service", 999));
    assert_eq!(duplicate(&library, &candidate).unwrap().id, "saved");
    candidate.steps[0]
        .routes
        .insert("failed".into(), "@next".into());
    assert!(duplicate(&library, &candidate).is_none());
}

#[test]
fn score_rewards_success_reuse_and_keep_penalizes_failures_and_decays() {
    let mut item = entry("a", 2);
    item.successes = 3;
    item.failures = 1;
    item.kept = true;
    assert_eq!(score(&item, 100), 11.0);
    assert_eq!(score(&item, 100 + 30 * 86400), 5.5);
    let library = (0..51)
        .map(|index| entry(&format!("item-{index:02}"), index))
        .collect::<Vec<_>>();
    assert_eq!(evictions(&library, 50, 100, "saved"), vec!["item-00"]);
}

#[test]
fn recall_ranks_problem_before_popularity_and_rejects_unrelated_items() {
    let ci = entry("ci", 1);
    let mut health = entry("health", 500);
    health.intents = vec!["Watch HTTP health".into()];
    assert_eq!(
        recall(&[health.clone(), ci.clone()], "CI failing jobs", 3, 100)[0].id,
        "ci"
    );
    assert!(recall(&[health, ci], "transcribe audio", 3, 100).is_empty());
}

#[test]
fn dreamer_merges_inputs_statistics_and_best_problem_with_hard_budget() {
    let a = entry("a", 5);
    let mut b = entry("b", 1);
    b.intents = vec!["Watch a CI run and report failed jobs before completion".into()];
    b.document.inputs.insert(
        "interval".into(),
        Input {
            kind: "string".into(),
            default: Some(json!("1m")),
            description: Some("Probe cadence".into()),
            choices: None,
            ..Default::default()
        },
    );
    let merged = merge_pair(&a, &b).unwrap();
    assert_eq!(merged.keeper.id, "a");
    assert_eq!(merged.keeper.uses, 6);
    assert_eq!(merged.keeper.successes, 2);
    assert!(merged.keeper.document.inputs.contains_key("interval"));
    assert_eq!(
        merged.keeper.intents,
        vec![
            "Watch CI run and report failing jobs".to_owned(),
            "Watch a CI run and report failed jobs before completion".to_owned()
        ]
    );
    let entries = (0..50)
        .map(|index| entry(&format!("{index}"), 1))
        .collect::<Vec<_>>();
    assert!(dream_merges(&entries, 0).is_empty());
    assert_eq!(dream_merges(&entries, usize::MAX).len(), MERGE_BUDGET);
}

#[test]
fn intents_accept_one_to_five_plain_values() {
    let intents = |n: usize| (0..n).map(|i| format!("intent {i}")).collect::<Vec<_>>();
    assert!(validate_intents(&intents(0)).is_err());
    assert_eq!(validate_intents(&intents(5)).unwrap().len(), 5);
    assert!(validate_intents(&intents(6)).is_err());
    assert!(validate_intents(&["bad\u{7}".into()]).is_err());
    assert!(validate_intents(&["x".repeat(201)]).is_err());
    assert_eq!(
        validate_intents(&["  watch   CI ".into()]).unwrap(),
        vec!["watch CI".to_owned()]
    );
    assert_eq!(
        merge_intents(&["a".into(), "B".into()], &["b".into(), "c".into()]),
        vec!["a".to_owned(), "B".to_owned(), "c".to_owned()]
    );
}
