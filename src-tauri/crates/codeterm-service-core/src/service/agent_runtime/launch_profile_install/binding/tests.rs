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
        ("askScope", json!("outsideWorkspace")),
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
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../../../../plugins/provider-acp/tests/fixtures/earlier-install-receipts.json"
    ))
    .unwrap();
    for install in fixture["registryInstalls"].as_array().unwrap() {
        let recorded = install["receiptSha256"].as_str().unwrap();
        let mut profile: RuntimeLaunchProfile =
            serde_json::from_value(install["declaration"].clone()).unwrap();
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
