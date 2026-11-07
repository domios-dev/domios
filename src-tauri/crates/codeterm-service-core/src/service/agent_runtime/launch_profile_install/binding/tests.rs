use super::*;
use serde_json::{json, Value};

fn registry_declaration() -> Value {
    json!({
        "registryId":"fixture-acp","iconUrl":"https://example.test/fixture.svg",
        "binary":"npx","args":["@fixture/agent@1.2.3"],
        "install":{"distribution":"npx","package":"@fixture/agent@1.2.3"},"ownerConfigured":true
    })
}

fn declared(change: impl FnOnce(&mut serde_json::Map<String, Value>)) -> RuntimeLaunchProfile {
    let mut declaration = registry_declaration();
    change(declaration.as_object_mut().unwrap());
    serde_json::from_value(declaration).unwrap()
}

#[test]
fn install_identity_ignores_presentation_policy_and_overlays_but_tracks_the_install() {
    let installed = fingerprint(&declared(|_| {})).unwrap();
    let mut projected = declared(|_| {});
    projected.icon_svg = Some("host projection".into());
    assert_eq!(fingerprint(&projected).unwrap(), installed);
    for (field, value) in [
        ("icon", json!("fixture")),
        ("registryId", json!("other-acp")),
        ("iconUrl", json!("https://example.test/other.svg")),
        ("env", json!({"FIXTURE_MODE":"fixture"})),
        ("hostWorkingDirectory", json!(true)),
        (
            "secretEnv",
            json!({"FIXTURE_TOKEN":"domios:provider:acp:fixture"}),
        ),
        (
            "authEnv",
            json!({"FIXTURE_KEY":{"secret":"domios:provider:acp:fixture-key","methods":["fixture"]}}),
        ),
        ("ownerConfigured", json!(false)),
        ("detection", json!({"args":["--fixture"]})),
        ("clientCapabilities", json!({"terminal":false})),
        (
            "limitsSource",
            json!({"kind":"account","provider":"fixture"}),
        ),
        ("fullAccessMode", json!("fixture-approve-all")),
    ] {
        let profile = declared(|declaration| {
            declaration.insert(field.into(), value);
        });
        assert_eq!(fingerprint(&profile).unwrap(), installed, "{field}");
    }
    for (field, value) in [
        ("binary", json!("fixture-agent")),
        ("args", json!(["@fixture/agent@1.2.3", "--other"])),
        (
            "install",
            json!({"distribution":"npx","package":"@fixture/agent@1.2.4"}),
        ),
        (
            "install",
            json!({"distribution":"uvx","package":"@fixture/agent@1.2.3"}),
        ),
        (
            "install",
            json!({"package":"@fixture/agent@1.2.3","registry":"https://registry.npmjs.org"}),
        ),
        (
            "install",
            json!({"distribution":"binary","package":"@fixture/agent@1.2.3","url":"https://example.test/agent","sha256":"a".repeat(64),"executable":"agent"}),
        ),
    ] {
        let profile = declared(|declaration| {
            declaration.insert(field.into(), value.clone());
        });
        assert_ne!(
            fingerprint(&profile).unwrap(),
            installed,
            "{field}: {value}"
        );
    }
}

#[test]
fn receipts_hashed_by_earlier_builds_keep_their_binding_and_ignore_later_fields() {
    // Declarations and hashes exactly as an earlier build stored them for registry installs.
    for (declaration, recorded) in [
        (
            r#"{"registryId":"claude-acp","iconUrl":"https://cdn.agentclientprotocol.com/registry/v1/latest/claude-acp.svg","binary":"npx","args":["@agentclientprotocol/claude-agent-acp@0.101.0"],"install":{"distribution":"npx","package":"@agentclientprotocol/claude-agent-acp@0.101.0"},"ownerConfigured":true}"#,
            "b6739036e49f035d0494a742bc2b215448c087a30e899d463a6c4903ab2c701b",
        ),
        (
            r#"{"registryId":"codex-acp","iconUrl":"https://cdn.agentclientprotocol.com/registry/v1/latest/codex-acp.svg","binary":"npx","args":["@agentclientprotocol/codex-acp@2.5.0"],"install":{"distribution":"npx","package":"@agentclientprotocol/codex-acp@2.5.0"},"ownerConfigured":true}"#,
            "668f922cb398a0fc0cdca5487864caaeffea6d99d7cabdb8f7f927911298d9bc",
        ),
    ] {
        let mut profile: RuntimeLaunchProfile = serde_json::from_str(declaration).unwrap();
        assert_eq!(
            bound_by(&profile, recorded).unwrap(),
            Some(Binding::LegacyDeclaration)
        );
        profile.icon_svg = Some("host projection".into());
        profile.full_access_mode = Some("fixture-approve-all".into());
        profile.limits_source =
            serde_json::from_value(json!({"kind":"account","provider":"fixture"})).unwrap();
        profile.host_working_directory = true;
        assert_eq!(
            bound_by(&profile, recorded).unwrap(),
            Some(Binding::LegacyDeclaration)
        );
        assert_eq!(
            bound_by(&profile, &fingerprint(&profile).unwrap()).unwrap(),
            Some(Binding::InstallIdentity)
        );
        profile.args.push("--other".into());
        assert_eq!(bound_by(&profile, recorded).unwrap(), None);
    }
}
