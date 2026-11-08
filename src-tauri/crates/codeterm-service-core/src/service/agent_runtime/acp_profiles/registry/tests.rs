use super::*;

#[test]
fn registry_failures_keep_a_bounded_cause_without_control_characters() {
    assert!(failure("unavailable", "HTTP 503 Service Unavailable").contains("HTTP 503"));
    assert!(failure(
        "unavailable",
        format!("request timed out ({FETCH_SECONDS}s deadline)")
    )
    .contains("20s deadline"));
    let message = failure(
        "unavailable",
        format!("connection refused\r\n{}", "x".repeat(1000)),
    );
    assert!(!message.chars().any(char::is_control));
    assert!(message.len() < 300);
    let parse = decode("{broken").err().unwrap();
    assert!(parse.contains("invalid JSON"));
    assert!(parse.contains("line 1"));
}

fn fixture() -> Index {
    decode(include_str!("fixture.json")).unwrap()
}

#[test]
fn fixture_maps_pinned_packages_binary_checksums_and_icon_identity() {
    let index = fixture();
    let npm = mapped(&index, "fixture-npm", "windows-x86_64").unwrap();
    assert_eq!(
        npm.install.as_ref().unwrap().package,
        "@fixture/agent@1.2.3"
    );
    assert_eq!(npm.args, ["@fixture/agent@1.2.3", "--stdio"]);
    assert_eq!(npm.env["MODE"], "fixture");
    assert_eq!(npm.registry_id.as_deref(), Some("fixture-npm"));
    assert_eq!(
        npm.icon_url.as_deref(),
        Some("https://example.test/icon.svg")
    );
    let python = mapped(&index, "fixture-python", "linux-x86_64").unwrap();
    assert_eq!(
        python.install.as_ref().unwrap().package,
        "fixture-agent==2.3.4"
    );
    assert_eq!(python.binary, "uvx");
    let binary = mapped(&index, "fixture-binary", "windows-x86_64").unwrap();
    assert_eq!(
        binary.install.as_ref().unwrap().sha256.as_deref(),
        Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
    );
    assert!(mapped(&index, "fixture-binary", "linux-x86_64").is_err());
    assert!(mapped(&index, "missing", "windows-x86_64").is_err());
}

#[test]
fn registry_refuses_tags_conflicting_pins_reserved_environment_and_missing_sha256() {
    let original: Value = serde_json::from_str(include_str!("fixture.json")).unwrap();
    for (id, agent, kind, field, bad) in [
        (
            "fixture-npm",
            0,
            "npx",
            "package",
            json!("@fixture/agent@latest"),
        ),
        (
            "fixture-npm",
            0,
            "npx",
            "package",
            json!("@fixture/agent@9.9.9"),
        ),
        (
            "fixture-python",
            1,
            "uvx",
            "package",
            json!("fixture-agent>=1.0"),
        ),
        (
            "fixture-npm",
            0,
            "npx",
            "env",
            json!({"CODETERM_TOKEN":"bad"}),
        ),
    ] {
        let mut value = original.clone();
        value["agents"][agent]["distribution"][kind][field] = bad;
        let index = decode(&value.to_string()).unwrap();
        assert!(mapped(&index, id, "windows-x86_64").is_err());
    }
    let mut value = original;
    value["agents"][2]["distribution"]["binary"]["windows-x86_64"]["sha256"] = Value::Null;
    assert!(mapped(
        &decode(&value.to_string()).unwrap(),
        "fixture-binary",
        "windows-x86_64"
    )
    .is_err());
}

#[test]
fn one_malformed_entry_leaves_the_other_agents_listed() {
    let mut value: Value = serde_json::from_str(include_str!("fixture.json")).unwrap();
    value["agents"][0]["version"] = json!("bad version\n");
    value["agents"][1]["version"] = json!("2026.11.05");
    value["agents"][2]["unexpected"] = json!(true);
    let index = decode(&value.to_string()).unwrap();
    let ids: Vec<&str> = index.agents.iter().map(|agent| agent.id.as_str()).collect();
    assert!(!ids.contains(&"fixture-npm"));
    assert!(ids.contains(&"fixture-python"));
    assert!(ids.contains(&"fixture-binary"));
}
