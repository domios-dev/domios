use std::cell::Cell;
use std::collections::BTreeMap;

use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{PendingPrompt, PromptOption};
use crate::sentinel::provenance::{Classification, Scope};
use crate::sentinel::store::{Create, Owner, Store};

fn fixture_prompt() -> PendingPrompt {
    PendingPrompt {
        question: "password=prompt-secret".into(),
        options: vec![PromptOption {
            label: "Continue".into(),
            value: "https://user:pass@example.test/path?token=query-secret".into(),
        }],
        sections: None,
        multi_select: Some(false),
        submit_available: None,
        selected_values: None,
    }
}

#[test]
fn sentinel_observe_json_is_canonical_redacted_and_screen_bounded() {
    let screen = format!("{} token=screen-secret", "screen ".repeat(800));
    let first = build_observation(
        Some(&fixture_prompt()),
        Some(codeterm_types::pty::PromptKind::Provider),
        "question",
        false,
        None,
        Some(&screen),
    )
    .unwrap();
    let second = build_observation(
        Some(&fixture_prompt()),
        Some(codeterm_types::pty::PromptKind::Provider),
        "question",
        false,
        None,
        Some(&screen),
    )
    .unwrap();
    let encoded = serde_json::to_string(&first).unwrap();
    assert_eq!(encoded, serde_json::to_string(&second).unwrap());
    assert!(!encoded.contains("provider_id"));
    assert!(!encoded.contains("fixture-provider"));
    assert!(!encoded.contains("prompt-secret"));
    assert!(!encoded.contains("query-secret"));
    assert!(!encoded.contains("screen-secret"));
    assert!(first["screen_tail"].as_str().unwrap().len() <= MAX_SCREEN_BYTES);
    assert_eq!(first["prompt"]["kind"], "provider");
}

#[test]
fn sentinel_observe_source_requires_agent_in_creator_team_workspace() {
    let (mut mux, creator) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&creator).unwrap();
    let bound = mux.create_tab(&workspace).unwrap();
    let other_workspace = mux.create_workspace("Other".into(), None).id;
    let foreign = mux.create_tab(&other_workspace).unwrap();
    for id in [&bound, &foreign] {
        let tab = mux.get_tab_mut(id).unwrap();
        tab.agent_type = Some("fixture-provider".into());
        tab.team_id = Some("creator-team".into());
    }
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(
        dir.path().join("sentinels.json"),
        crate::cron::clock::MockClock::starting_at(1_000),
    );
    let job = store
        .prepare(
            Create {
                name: None,
                probe: None,
                observe: Some(TabBinding::Tab(bound.clone())),
                question: "state?".into(),
                buckets: vec!["waiting".into()],
                every: "10s".into(),
                routes: BTreeMap::new(),
                matches: BTreeMap::new(),
                until: None,
                timeout: None,
                paused: false,
                handlers: None,
                max_transitions: None,
                max_actions: None,
            },
            Owner {
                creator_tab_id: creator,
                team_id: Some("creator-team".into()),
                workspace_id: workspace,
                cwd: ".".into(),
            },
            true,
        )
        .unwrap();
    assert!(validate_tab_scope(&mux, &job, &bound).is_ok());
    assert!(validate_tab_scope(&mux, &job, &foreign).is_err());
    mux.get_tab_mut(&bound).unwrap().team_id = None;
    assert!(validate_tab_scope(&mux, &job, &bound).is_err());
    mux.get_tab_mut(&bound).unwrap().team_id = Some("creator-team".into());
    mux.get_tab_mut(&bound).unwrap().agent_type = None;
    assert!(validate_tab_scope(&mux, &job, &bound).is_err());
}

#[test]
fn sentinel_observe_unchanged_tab_observation_makes_no_second_classify_call() {
    let value = build_observation(
        Some(&fixture_prompt()),
        Some(codeterm_types::pty::PromptKind::Provider),
        "question",
        true,
        None,
        None,
    )
    .unwrap();
    let bytes = serde_json::to_vec(&value).unwrap();
    let mut machine = super::super::machine::Machine::default();
    let calls = Cell::new(0);
    for _ in 0..2 {
        super::super::machine::evaluate(
            &mut machine,
            &bytes,
            &["waiting".into()],
            &BTreeMap::new(),
            None,
            Scope {
                sentinel_id: "watch".into(),
                creator_tab_id: "creator".into(),
                team_id: Some("team".into()),
                workspace_id: "workspace".into(),
            },
            || {
                calls.set(calls.get() + 1);
                Classification::adapter(Ok(super::super::machine::Verdict {
                    bucket: "waiting".into(),
                    confidence: 0.9,
                }))
            },
        );
    }
    assert_eq!(calls.get(), 1);
}

#[test]
fn sentinel_observe_source_scan_has_no_pty_writer() {
    fn read_sources(root: &std::path::Path, output: &mut Vec<String>) {
        for entry in std::fs::read_dir(root).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                read_sources(&path, output);
            } else if path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension == "rs")
            {
                output.push(std::fs::read_to_string(path).unwrap());
            }
        }
    }

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut sources = Vec::new();
    for directory in ["src/sentinel", "src/service/sentinel"] {
        read_sources(&root.join(directory), &mut sources);
    }
    let writers = [
        ["write_bound_prompt", "_steps"].concat(),
        ["pty", ".lock().", "write"].concat(),
        ["send", "_text"].concat(),
        ["service::pty", "_io"].concat(),
    ];
    assert!(sources
        .iter()
        .all(|source| writers.iter().all(|writer| !source.contains(writer))));
}
