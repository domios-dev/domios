use super::*;
use serde_json::{json, Value};

fn profile(value: Value) -> RuntimeLaunchProfile {
    serde_json::from_value(value).unwrap()
}

#[test]
fn exact_package_versions_exclude_tags_ranges_urls_and_options() {
    for value in [
        "agent",
        "agent@latest",
        "agent@^1.2.3",
        "../agent@1.2.3",
        "@agent@1.2.3",
        "--option@1.2.3",
        "https://host@1.2.3",
    ] {
        assert!(!exact_npm(value), "{value}");
    }
    assert!(exact_npm("agent@1.2.3"));
    assert!(exact_npm("@scope/agent@1.2.3-rc.1"));
    for value in [
        "agent",
        "agent>=1.2.3",
        "agent==*",
        "agent==1.*",
        "agent==https://host",
        "--agent==1.0",
    ] {
        assert!(exact_python(value).is_none(), "{value}");
    }
    assert_eq!(
        exact_python("agent-cli==1.2.3"),
        Some(("agent-cli", "1.2.3"))
    );
}

#[test]
fn launchers_require_the_matching_pinned_install_declaration() {
    for binary in ["npx", "npm.cmd", "/usr/bin/uvx", "C:\\tools\\env.exe", "uv"] {
        assert!(!validate(&profile(
            json!({"binary":binary,"args":[],"ownerConfigured":true})
        )));
    }
    for (binary, pin) in [("npx", "agent@1.2.3"), ("uvx", "agent==1.2.3")] {
        let mut value = json!({"binary":binary,"args":[pin,"--stdio"],"install":{"distribution":binary,"package":pin},"ownerConfigured":true});
        assert!(validate(&profile(value.clone())));
        value["args"][0] = "different".into();
        assert!(!validate(&profile(value)));
    }
}

#[test]
fn binary_distribution_requires_https_sha256_and_a_safe_executable() {
    let value = json!({"binary":"bin/agent","args":[],"install":{"distribution":"binary","package":"agent@1.2.3","url":"https://example.test/agent.tgz","sha256":"a".repeat(64),"executable":"bin/agent"},"ownerConfigured":true});
    assert!(validate(&profile(value.clone())));
    for (field, bad) in [
        ("sha256", json!(null)),
        ("sha256", json!("latest")),
        ("url", json!("http://example.test/a")),
        ("url", json!("https://user:password@example.test/a")),
        ("executable", json!("../agent")),
        ("executable", json!("/agent")),
        ("executable", json!("NUL.exe")),
    ] {
        let mut bad_value = value.clone();
        bad_value["install"][field] = bad;
        assert!(!validate(&profile(bad_value)), "{field}");
    }
}
