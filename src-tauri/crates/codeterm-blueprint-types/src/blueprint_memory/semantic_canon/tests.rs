use super::*;
use crate::blueprint_memory::parse_document;
const INTENT: &str = "Halt a release when the essence map breaks";
const BASE: &str = r#"schema_version: 2
kind: pipeline
name: release-gate
note: Block a release until the map is valid.
inputs:
  root: {type: string, description: Canonical checkout}
trigger: {type: schedule, every: {lit: 1h}}
steps:
- {id: map-check, run: {lit: "codeterm essence-map validate --root '{{root}}'"}, routes: {failed: blocked}}
- {id: health, uses: http, with: {url: {lit: "https://status.example.com/api/health"}}, routes: {down: blocked}}
- {id: verdict, uses: filter, with: {pattern: {lit: '(?m)^ok$'}, bucket: {lit: green}}, routes: {green: '@next', failed: blocked}}
- {id: blocked, run: {lit: echo release-blocked}}
result: {to: notify}
"#;
const WATCH: &str = r#"schema_version: 2
kind: watch
name: ci-watch
note: Tell me when CI turns red.
inputs:
  pr_probe: {type: string, description: Read-only gh pr checks command}
steps:
- {id: source, uses: probe, with: {command: {input: pr_probe}}, every: {lit: 1m}}
- id: decide
  uses: ask
  with: {question: {lit: is-ci-red}}
  buckets: [zebrabucket, yakbucket]
  matches: {zebrabucket: {lit: '(?m)\bfail\b'}, yakbucket: {lit: '(?m)^All checks were successful'}}
  until: yakbucket
  routes: {zebrabucket: send, yakbucket: success}
other:
- {id: send, uses: send, with: {target: {lit: 'agent:fixer'}, text: {lit: Please look at the failing job}}, routes: {done: notify}}
- {id: notify, uses: notify, with: {text: {lit: CI went red}}}
- {id: success, uses: notify}
"#;
fn canon(source: &str) -> String {
    semantic_canon(&parse_document(source).unwrap(), INTENT)
}
#[test]
fn golden_pipeline_and_watch() {
    assert_eq!(canon(BASE), "halt a release when the essence map breaks. block a release until the map is valid. trigger: schedule every 1h. inputs: canonical checkout. map check: runs codeterm essence map validate root {root}; health: get {host} api health; verdict: matches ok; blocked: runs echo release blocked.");
    let text = canon(WATCH);
    for meaning in [
        "source: runs read-only gh pr checks command",
        "decide: asks is ci red",
        "matches fail",
        "matches all checks were successful",
        "send: send agent please look at the failing job",
        "notify: notify ci went red",
    ] {
        assert!(text.contains(meaning), "{meaning} absent: {text}");
    }
}
#[test]
fn twenty_formatting_perturbations_keep_canon_and_hash_byte_identical() {
    let document = parse_document(BASE).unwrap();
    let json = serde_json::to_value(&document).unwrap();
    let pretty = serde_json::to_string_pretty(&json).unwrap();
    let mut variants = vec![
        serde_json::to_string(&json).unwrap(),
        pretty,
        BASE.replace('\n', "\r\n"),
        BASE.replace(
            "Block a release until the map is valid.",
            ">-\n  Block a release\n  until the map is valid.",
        ),
        BASE.replace(
            "Block a release until the map is valid.",
            "|-\n  Block a release until the map is valid.",
        ),
    ];
    for index in 0..15 {
        variants.push(format!("# format variation {index}\n---\n{BASE}\n"));
    }
    assert_eq!(variants.len(), 20);
    let expected = canon(BASE);
    let expected_json = canonical_json(&document);
    for source in variants {
        assert_ne!(source, BASE);
        assert_eq!(canon(&source), expected);
        assert_eq!(
            canonical_json(&parse_document(&source).unwrap()),
            expected_json
        );
    }
}
#[test]
fn ten_semantic_mutations_each_change_canon() {
    let mutations = [
        ("essence-map validate", "essence-map check"),
        ("Block a release", "Block a deploy"),
        ("lit: 1h", "lit: 1d"),
        ("api/health", "api/ready"),
        ("(?m)^ok$", "(?m)^passed$"),
        ("Canonical checkout", "Release worktree"),
        ("echo release-blocked", "echo deployment-blocked"),
        ("id: health", "id: probe-health"),
        ("uses: http", "uses: github"),
        (
            "type: schedule, every: {lit: 1h}",
            "type: event, kind: {lit: tab_idle}",
        ),
    ];
    let mut seen = std::collections::BTreeSet::from([canon(BASE)]);
    for (from, to) in mutations {
        assert!(BASE.contains(from));
        let source = BASE.replacen(from, to, 1);
        assert!(seen.insert(canon(&source)), "{from} -> {to}");
    }
    assert!(seen.insert(semantic_canon(
        &parse_document(BASE).unwrap(),
        "Another intent"
    )));
}
#[test]
fn canon_carries_no_topology_wording() {
    let step_number = regex::Regex::new(r"step \d").unwrap();
    for source in [BASE, WATCH] {
        let text = canon(source);
        for banned in [
            " then ",
            "routes to",
            "route",
            "next",
            "stop",
            "done",
            "failed",
            "down",
            "green",
            "zebrabucket",
            "yakbucket",
            "other",
            "@",
            "->",
        ] {
            assert!(!text.contains(banned), "{banned:?} in {text}");
        }
        assert!(!step_number.is_match(&text), "step numbering in {text}");
    }
}
#[test]
fn canon_drops_credentials_and_instance_paths() {
    let mut document = parse_document(BASE).unwrap();
    document.steps[3].run=Some(crate::blueprint_document::ValueForm::literal("curl -H 'Authorization: Bearer ghp_abcdefghijklmnopqrstuvwxyz0123456789' /Users/me/secret.txt"));
    let text = semantic_canon(&document, INTENT);
    for leaked in ["ghp_", "/users", "authorization"] {
        assert!(!text.contains(leaked), "{text}");
    }
    assert!(text.contains("runs curl h"), "{text}");
}
#[test]
fn canon_drops_secrets_that_follow_a_credential_keyword() {
    for command in [
        "deploy --password hunter-two-secret --token abcdefghij && curl -H 'Authorization: Bearer zyxwvutsrqponmlkjihgfedcba' https://x.example.com",
        "deploy --password 'hunter two secret' --token abcdefghij && curl -H 'Authorization: Bearer shortvalue' https://x.example.com",
        "deploy --password=abcdefghi --token abcdefghij && curl --header=\"Authorization: Bearer shortvalue\" https://x.example.com",
        "deploy --password hunter-two-secret --token abcdefghij && curl -H 'Authorization: {{token}}' https://x.example.com",
    ] {
        let mut document=parse_document(BASE).unwrap();document.steps[0].run=Some(crate::blueprint_document::ValueForm::literal(command));
        let text=semantic_canon(&document,INTENT);
        for leaked in ["hunter","abcdefgh","zyxwvutsrqponmlkjihgfedcba","shortvalue","password","token","authorization","bearer","secret"] {assert!(!text.contains(leaked),"{leaked} in {text}");}
        assert!(text.contains("runs deploy curl"),"{text}");assert!(text.contains("{url}"),"{text}");
    }
}
