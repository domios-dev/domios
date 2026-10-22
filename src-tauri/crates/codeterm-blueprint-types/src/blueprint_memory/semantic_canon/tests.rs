use super::*;
use crate::blueprint_memory::parse_document;

const INTENT: &str = "Halt a release when the essence map breaks";

const BASE: &str = r#"schema_version: 1
name: release-gate
note: Block a release until the map is valid.
inputs:
  root: {type: string, description: Canonical checkout}
pipeline:
  trigger: {type: schedule, every: 1h}
  steps:
    - {name: map-check, input: text, run: "codeterm essence-map validate --root '{{root}}'", on: {failed: blocked}}
    - {name: health, uses: http, with: {url: "https://status.example.com/api/health"}, on: {down: blocked}}
    - {name: verdict, uses: filter, with: {pattern: '(?m)^ok$', bucket: green}, on: {green: next, failed: blocked}}
    - {name: blocked, run: echo release-blocked}
  result: {to: notify}
"#;

const WATCH: &str = r#"schema_version: 1
name: ci-watch
note: Tell me when CI turns red.
inputs:
  pr_probe: {type: string, description: Read-only gh pr checks command}
watch:
  probe: '{{pr_probe}}'
  question: is-ci-red
  buckets: [zebrabucket, yakbucket]
  matches: {zebrabucket: '(?m)\bfail\b', yakbucket: '(?m)^All checks were successful'}
  every: 1m
  until: yakbucket
  routes: {yakbucket: notify}
  handlers:
    zebrabucket:
      - {type: send, parameters: {target: 'agent:fixer', text: Please look at the failing job}}
      - {type: notify, parameters: {text: CI went red}}
"#;

fn canon(source: &str) -> String {
    semantic_canon(&parse_document(source).unwrap(), INTENT)
}

#[test]
fn golden_pipeline_and_watch() {
    assert_eq!(
        canon(BASE),
        "halt a release when the essence map breaks. block a release until the map is valid. \
         trigger: schedule every 1h. inputs: canonical checkout. map check: runs codeterm essence map validate root {root}; \
         health: get {host} api health; verdict: matches ok; blocked: runs echo release blocked."
    );
    assert_eq!(
        canon(WATCH),
        "halt a release when the essence map breaks. tell me when ci turns red. \
         trigger: checks every 1m. inputs: read-only gh pr checks command. source: runs read-only gh pr checks command; \
         decide: asks is ci red, matches all checks were successful, matches fail; \
         send: send, agent, please look at the failing job; notify: notify, ci went red; notify."
    );
}

const PERTURBATIONS: [&str; 20] = [
    "# leading comment\n{BASE}",
    "{BASE}\n\n\n",
    "---\n{BASE}",
    "{BASE}# trailing comment\n",
    "{BASE_BLANK}",
    r#"name: release-gate
schema_version: 1
inputs:
  root: {type: string, description: Canonical checkout}
note: Block a release until the map is valid.
pipeline:
  steps:
    - {name: map-check, input: text, run: "codeterm essence-map validate --root '{{root}}'", on: {failed: blocked}}
    - {name: health, uses: http, with: {url: "https://status.example.com/api/health"}, on: {down: blocked}}
    - {name: verdict, uses: filter, with: {pattern: '(?m)^ok$', bucket: green}, on: {green: next, failed: blocked}}
    - {name: blocked, run: echo release-blocked}
  result: {to: notify}
  trigger: {type: schedule, every: 1h}
"#,
    r#"schema_version: 1
name: release-gate
note: Block a release until the map is valid.
inputs:
  root:
    type: string
    description: Canonical checkout
pipeline:
  trigger:
    type: schedule
    every: 1h
  steps:
    - name: map-check
      input: text
      run: "codeterm essence-map validate --root '{{root}}'"
      on:
        failed: blocked
    - name: health
      uses: http
      with:
        url: "https://status.example.com/api/health"
      on:
        down: blocked
    - name: verdict
      uses: filter
      with:
        pattern: '(?m)^ok$'
        bucket: green
      on:
        green: next
        failed: blocked
    - name: blocked
      run: echo release-blocked
  result:
    to: notify
"#,
    r#"schema_version: 1
name: "release-gate"
note: "Block a release until the map is valid."
inputs:
  root: {type: "string", description: "Canonical checkout"}
pipeline:
  trigger: {type: "schedule", every: "1h"}
  steps:
    - {name: "map-check", input: "text", run: "codeterm essence-map validate --root '{{root}}'", on: {failed: "blocked"}}
    - {name: "health", uses: "http", with: {url: "https://status.example.com/api/health"}, on: {down: "blocked"}}
    - {name: "verdict", uses: "filter", with: {pattern: "(?m)^ok$", bucket: "green"}, on: {green: "next", failed: "blocked"}}
    - {name: "blocked", run: "echo release-blocked"}
  result: {to: "notify"}
"#,
    r#"schema_version: 1
name: 'release-gate'
note: 'Block a release until the map is valid.'
inputs:
  root: {type: 'string', description: 'Canonical checkout'}
pipeline:
  trigger: {type: 'schedule', every: '1h'}
  steps:
    - {name: 'map-check', input: 'text', run: 'codeterm essence-map validate --root ''{{root}}''', on: {failed: 'blocked'}}
    - {name: 'health', uses: 'http', with: {url: 'https://status.example.com/api/health'}, on: {down: 'blocked'}}
    - {name: 'verdict', uses: 'filter', with: {pattern: '(?m)^ok$', bucket: 'green'}, on: {green: 'next', failed: 'blocked'}}
    - {name: 'blocked', run: 'echo release-blocked'}
  result: {to: 'notify'}
"#,
    r#"schema_version: 1
name: release-gate
note: >-
  Block a release
  until the map is valid.
inputs:
  root: {type: string, description: Canonical checkout}
pipeline:
  trigger: {type: schedule, every: 1h}
  steps:
    - {name: map-check, input: text, run: "codeterm essence-map validate --root '{{root}}'", on: {failed: blocked}}
    - {name: health, uses: http, with: {url: "https://status.example.com/api/health"}, on: {down: blocked}}
    - {name: verdict, uses: filter, with: {pattern: '(?m)^ok$', bucket: green}, on: {green: next, failed: blocked}}
    - {name: blocked, run: echo release-blocked}
  result: {to: notify}
"#,
    r#"schema_version: 1
name: release-gate
note: Block a release until the map is valid.
inputs:
    root: {type: string, description: Canonical checkout}
pipeline:
    trigger: {type: schedule, every: 1h}
    steps:
        - {name: map-check, input: text, run: "codeterm essence-map validate --root '{{root}}'", on: {failed: blocked}}
        - {name: health, uses: http, with: {url: "https://status.example.com/api/health"}, on: {down: blocked}}
        - {name: verdict, uses: filter, with: {pattern: '(?m)^ok$', bucket: green}, on: {green: next, failed: blocked}}
        - {name: blocked, run: echo release-blocked}
    result: {to: notify}
"#,
    r#"schema_version: 1
name: release-gate
note: Block a release until the map is valid.
inputs:
  root: {description: Canonical checkout, type: string}
pipeline:
  trigger: {every: 1h, type: schedule}
  steps:
    - {run: "codeterm essence-map validate --root '{{root}}'", on: {failed: blocked}, input: text, name: map-check}
    - {with: {url: "https://status.example.com/api/health"}, uses: http, name: health, on: {down: blocked}}
    - {on: {failed: blocked, green: next}, with: {bucket: green, pattern: '(?m)^ok$'}, name: verdict, uses: filter}
    - {run: echo release-blocked, name: blocked}
  result: {to: notify}
"#,
    r#"{"schema_version": 1, "name": "release-gate", "note": "Block a release until the map is valid.", "inputs": {"root": {"type": "string", "description": "Canonical checkout"}}, "pipeline": {"trigger": {"type": "schedule", "every": "1h"}, "steps": [{"name": "map-check", "input": "text", "run": "codeterm essence-map validate --root '{{root}}'", "on": {"failed": "blocked"}}, {"name": "health", "uses": "http", "with": {"url": "https://status.example.com/api/health"}, "on": {"down": "blocked"}}, {"name": "verdict", "uses": "filter", "with": {"pattern": "(?m)^ok$", "bucket": "green"}, "on": {"green": "next", "failed": "blocked"}}, {"name": "blocked", "run": "echo release-blocked"}], "result": {"to": "notify"}}}
"#,
    "{BASE_TRAILING}",
    "{BASE_CRLF}",
    r#"schema_version: 1
name: release-gate
note: Block a release until the map is valid.
inputs: {root: {type: string, description: Canonical checkout}}
pipeline: {trigger: {type: schedule, every: 1h}, result: {to: notify}, steps: [{name: map-check, input: text, run: "codeterm essence-map validate --root '{{root}}'", on: {failed: blocked}}, {name: health, uses: http, with: {url: "https://status.example.com/api/health"}, on: {down: blocked}}, {name: verdict, uses: filter, with: {pattern: '(?m)^ok$', bucket: green}, on: {green: next, failed: blocked}}, {name: blocked, run: echo release-blocked}]}
"#,
    r#"schema_version: 1
name: release-gate
# the note is read by search
note: Block a release until the map is valid.
inputs:
  # checkout root
  root: {type: string, description: Canonical checkout}
pipeline:
  trigger: {type: schedule, every: 1h}  # hourly
  steps:
    # validation first
    - {name: map-check, input: text, run: "codeterm essence-map validate --root '{{root}}'", on: {failed: blocked}}
    - {name: health, uses: http, with: {url: "https://status.example.com/api/health"}, on: {down: blocked}}
    - {name: verdict, uses: filter, with: {pattern: '(?m)^ok$', bucket: green}, on: {green: next, failed: blocked}}
    - {name: blocked, run: echo release-blocked}
  result: {to: notify}
"#,
    r#"schema_version: 1
name: release-gate
note: |-
  Block a release until the map is valid.
inputs:
  root: {type: string, description: Canonical checkout}
pipeline:
  trigger: {type: schedule, every: 1h}
  steps:
    - {name: map-check, input: text, run: "codeterm essence-map validate --root '{{root}}'", on: {failed: blocked}}
    - {name: health, uses: http, with: {url: "https://status.example.com/api/health"}, on: {down: blocked}}
    - {name: verdict, uses: filter, with: {pattern: '(?m)^ok$', bucket: green}, on: {green: next, failed: blocked}}
    - {name: blocked, run: echo release-blocked}
  result: {to: notify}
"#,
    r#"schema_version: 1
name: release-gate
note: Block a release until the map is valid.
inputs:
  root: {type: string, description: Canonical checkout}
pipeline:
  trigger: {type: schedule, every: 1h}
  steps:
  - {name: map-check, input: text, run: "codeterm essence-map validate --root '{{root}}'", on: {failed: blocked}}
  - {name: health, uses: http, with: {url: "https://status.example.com/api/health"}, on: {down: blocked}}
  - {name: verdict, uses: filter, with: {pattern: '(?m)^ok$', bucket: green}, on: {green: next, failed: blocked}}
  - {name: blocked, run: echo release-blocked}
  result: {to: notify}
"#,
    r#"schema_version: 1
name: release-gate
note: Block a release until the map is valid.
inputs:
  root: {type: string, description: Canonical checkout}
pipeline:
  trigger: {type: schedule, every: 1h}
  steps:
    - {name: map-check, input: text, run: "codeterm essence-map validate --root '{{root}}'", on: {failed: blocked}}
    - {name: health, uses: http, with: {url: "https://status.example.com/api/health"}, on: {down: blocked}}
    - {name: verdict, uses: filter, with: {pattern: '(?m)^ok$', bucket: green}, on: {green: next, failed: blocked}}
    - {name: blocked, run: echo release-blocked}
  result:
    to: notify
"#,
];

fn expand(template: &str) -> String {
    template
        .replace("{BASE_CRLF}", &BASE.replace('\n', "\r\n"))
        .replace("{BASE_BLANK}", &BASE.replace("\n    - ", "\n\n    - "))
        .replace("{BASE_TRAILING}", &BASE.replace('\n', "   \n"))
        .replace("{BASE}", BASE)
}

#[test]
fn twenty_formatting_perturbations_keep_canon_and_hash_byte_identical() {
    let expected = canon(BASE);
    let expected_json = canonical_json(&parse_document(BASE).unwrap());
    let mut distinct = std::collections::BTreeSet::new();
    for (index, template) in PERTURBATIONS.iter().enumerate() {
        let source = expand(template);
        assert!(
            distinct.insert(source.clone()),
            "perturbation {index} repeats another"
        );
        assert_ne!(source, BASE, "perturbation {index} is not a perturbation");
    }
    for (index, template) in PERTURBATIONS.iter().enumerate() {
        let source = expand(template);
        assert_eq!(canon(&source), expected, "perturbation {index}");
        assert_eq!(
            canonical_json(&parse_document(&source).unwrap()),
            expected_json,
            "perturbation {index}"
        );
    }
}

const MUTATIONS: [(&str, &str); 10] = [
    ("essence-map validate", "essence-map check"),
    (
        "Block a release until the map is valid.",
        "Block a deploy until the map is valid.",
    ),
    ("every: 1h}", "every: 1d}"),
    (
        "https://status.example.com/api/health",
        "https://status.example.com/api/ready",
    ),
    ("pattern: '(?m)^ok$'", "pattern: '(?m)^passed$'"),
    (
        "description: Canonical checkout",
        "description: Release worktree",
    ),
    ("    - {name: blocked, run: echo release-blocked}\n", ""),
    ("name: health, uses: http", "name: probe-health, uses: http"),
    (
        "uses: http, with: {url:",
        "uses: github, with: {resource: run, url:",
    ),
    (
        "trigger: {type: schedule, every: 1h}",
        "trigger: {type: event, kind: tab_idle}",
    ),
];

#[test]
fn ten_semantic_mutations_each_change_canon() {
    let base = canon(BASE);
    let mut seen = std::collections::BTreeSet::from([base.clone()]);
    for (index, (from, to)) in MUTATIONS.iter().enumerate() {
        assert!(BASE.contains(from), "mutation {index} does not apply");
        let source = BASE.replacen(from, to, 1);
        let Ok(document) = parse_document(&source) else {
            panic!("mutation {index} does not parse");
        };
        let mutated = semantic_canon(&document, INTENT);
        assert!(
            seen.insert(mutated.clone()),
            "mutation {index} left canon unchanged: {mutated}"
        );
    }
    assert_ne!(
        semantic_canon(&parse_document(BASE).unwrap(), "Another intent"),
        base
    );
}

#[test]
fn canon_carries_no_topology_wording() {
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
        assert!(
            !regex::Regex::new(r"step \d").unwrap().is_match(&text),
            "step numbering in {text}"
        );
    }
}

#[test]
fn canon_drops_credentials_and_instance_paths() {
    let source = BASE.replace(
        "run: echo release-blocked",
        "run: \"curl -H 'Authorization: Bearer ghp_abcdefghijklmnopqrstuvwxyz0123456789' /Users/me/secret.txt\"",
    );
    let text = canon(&source);
    assert!(!text.contains("ghp_"), "{text}");
    assert!(!text.contains("/users"), "{text}");
    assert!(!text.contains("authorization"), "{text}");
    assert!(text.contains("runs curl h"), "{text}");
}

#[test]
fn canon_drops_secrets_that_follow_a_credential_keyword() {
    let source = BASE.replace(
        "run: echo release-blocked",
        "run: \"deploy --password hunter-two-secret --token abcdefghij && curl -H 'Authorization: Bearer zyxwvutsrqponmlkjihgfedcba' https://x.example.com\"",
    );
    let text = canon(&source);
    for leaked in [
        "hunter",
        "abcdefghij",
        "zyxwvutsrqponmlkjihgfedcba",
        "password",
        "token",
        "authorization",
        "bearer",
    ] {
        assert!(!text.contains(leaked), "{leaked} in {text}");
    }
    assert!(text.contains("runs deploy curl h {url}"), "{text}");
}
