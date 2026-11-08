use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};

#[test]
fn built_in_name_conflicts_do_not_suggest_removing_a_built_in() {
    assert_eq!(
        name_conflict("gemini-cli", false),
        "gemini-cli is a built-in profile; choose another name"
    );
    assert!(name_conflict("custom", true).contains("remove the owner profile"));
}

fn request(value: Value) -> Result<(String, RuntimeLaunchProfile), String> {
    parse_add(serde_json::from_value(value).unwrap(), None)
}

fn derived(value: Value, base: Value) -> Result<RuntimeLaunchProfile, String> {
    parse_add(
        serde_json::from_value(value).unwrap(),
        Some(serde_json::from_value(base).unwrap()),
    )
    .map(|(_, profile)| profile)
}

fn bundled_base() -> Value {
    json!({"binary":"base-agent","args":["--acp"],"env":{"BASE_ONLY":"1"},"icon":"base-color.svg",
        "authEnv":{"BASE_KEY":{"secret":"domios:provider:acp:base-key","methods":["base-key"]}},
        "clientCapabilities":{"fs":{"readTextFile":false}},
        "hostWorkingDirectory":true,
        "askScope":"outsideWorkspace",
        "limitsSource":{"kind":"unavailable","reason":"Base quota has no public endpoint"}})
}

#[test]
fn derived_profiles_copy_declared_identity_but_never_the_launch_command_or_credentials() {
    let profile = derived(
        json!({"name":"derived","binary":"derived-agent","args":["--lite"]}),
        bundled_base(),
    )
    .unwrap();
    assert_eq!(profile.binary, "derived-agent");
    assert_eq!(profile.args, ["--lite"]);
    assert!(profile.env.is_empty() && profile.auth_env.is_empty() && profile.secret_env.is_empty());
    assert!(profile.owner_configured);
    assert_eq!(profile.icon.as_deref(), Some("base-color.svg"));
    assert!(profile.host_working_directory);
    assert_eq!(
        serde_json::to_value(profile.ask_scope).unwrap(),
        json!("outsideWorkspace")
    );
    let fs = profile
        .client_capabilities
        .as_ref()
        .unwrap()
        .fs
        .as_ref()
        .unwrap();
    assert_eq!(fs.read_text_file, Some(false));
    assert_eq!(
        profile.limits_source,
        Some(RuntimeProfileLimitsSource::Unavailable {
            reason: "Base quota has no public endpoint".into()
        })
    );
    let registry = derived(
        json!({"name":"derived","binary":"derived-agent"}),
        json!({"binary":"npx","args":["fixture@1.0.0"],"ownerConfigured":true,"registryId":"fixture-agent",
            "iconUrl":"https://cdn.example.test/fixture.svg","install":{"distribution":"npx","package":"fixture@1.0.0"}}),
    )
    .unwrap();
    assert_eq!(registry.registry_id.as_deref(), Some("fixture-agent"));
    assert_eq!(
        registry.icon_url.as_deref(),
        Some("https://cdn.example.test/fixture.svg")
    );
    assert!(registry.install.is_none());
}

#[test]
fn client_capability_overrides_merge_over_the_base_and_follow_the_declaration_type() {
    let profile = derived(
        json!({"name":"derived","binary":"derived-agent","clientCapabilities":["fs.writeTextFile=false","terminal=true"]}),
        bundled_base(),
    )
    .unwrap();
    let capabilities = profile.client_capabilities.unwrap();
    let fs = capabilities.fs.unwrap();
    assert_eq!(
        (fs.read_text_file, fs.write_text_file),
        (Some(false), Some(false))
    );
    assert_eq!(capabilities.terminal, Some(true));
    let restored = derived(
        json!({"name":"derived","binary":"derived-agent","clientCapabilities":["fs.readTextFile=true"]}),
        bundled_base(),
    )
    .unwrap();
    assert_eq!(
        restored
            .client_capabilities
            .unwrap()
            .fs
            .unwrap()
            .read_text_file,
        Some(true)
    );
    let plain = request(json!({"name":"plain","binary":"plain-agent","clientCapabilities":["fs.readTextFile=false"]})).unwrap().1;
    assert_eq!(
        plain
            .client_capabilities
            .unwrap()
            .fs
            .unwrap()
            .read_text_file,
        Some(false)
    );
    for bad in [
        "fs.readTextFile",
        "fs.readTextFile=no",
        "fs.unknown=false",
        "fs=false",
        "terminal.fs=false",
        "fs.readTextFile.deep=false",
        ".readTextFile=false",
        "network=false",
    ] {
        assert!(
            request(json!({"name":"plain","binary":"plain-agent","clientCapabilities":[bad]}))
                .is_err(),
            "{bad}"
        );
    }
    assert!(request(json!({"name":"plain","binary":"plain-agent","clientCapabilities":["terminal=false","terminal=true"]})).is_err());
}

#[test]
fn limits_sources_parse_validate_against_the_profile_and_override_the_base() {
    let account = request(
        json!({"name":"plain","binary":"plain-agent","limitsSource":"account:native-fixture"}),
    )
    .unwrap()
    .1;
    assert_eq!(
        account.limits_source,
        Some(RuntimeProfileLimitsSource::Account {
            provider: "native-fixture".into()
        })
    );
    let balance = request(json!({"name":"plain","binary":"plain-agent","secretEnv":["KEY=domios:provider:acp:key"],"limitsSource":"balance:fixture-balance:KEY"})).unwrap().1;
    assert!(matches!(
        balance.limits_source,
        Some(RuntimeProfileLimitsSource::Balance { .. })
    ));
    let reason = request(json!({"name":"plain","binary":"plain-agent","limitsSource":"unavailable:Quota: no public endpoint"})).unwrap().1;
    assert_eq!(
        reason.limits_source,
        Some(RuntimeProfileLimitsSource::Unavailable {
            reason: "Quota: no public endpoint".into()
        })
    );
    for bad in [
        "native-fixture",
        "account:",
        "account:Two Words",
        "balance:fixture-balance",
        "balance:fixture-balance:UNDECLARED",
        "unavailable:",
        "guess:anything",
    ] {
        assert!(
            request(json!({"name":"plain","binary":"plain-agent","limitsSource":bad})).is_err(),
            "{bad}"
        );
    }
    let overridden = derived(
        json!({"name":"derived","binary":"derived-agent","limitsSource":"account:fixture-native"}),
        bundled_base(),
    )
    .unwrap();
    assert_eq!(
        overridden.limits_source,
        Some(RuntimeProfileLimitsSource::Account {
            provider: "fixture-native".into()
        })
    );
    assert!(
        derived(
            json!({"name":"derived","binary":"derived-agent"}),
            json!({"binary":"base-agent","args":[],"secretEnv":{"KEY":"domios:provider:acp:key"},
            "limitsSource":{"kind":"balance","reader":"fixture-balance","credentialEnv":"KEY"}}),
        )
        .is_err(),
        "an inherited balance needs the derived profile's own credential"
    );
}

#[test]
fn add_validates_names_environment_and_secret_references() {
    let valid = json!({"name":"fixture","binary":"fixture-agent","args":["--stdio"],"env":["MODE=test"],"secretEnv":["TOKEN=domios:provider:acp:fixture"],"authEnv":["KEY=domios:provider:acp:key:api-key"]});
    let (_, profile) = request(valid.clone()).unwrap();
    assert_eq!(profile.args, ["--stdio"]);
    assert_eq!(profile.secret_env["TOKEN"], "domios:provider:acp:fixture");
    assert_eq!(profile.auth_env["KEY"].methods, ["api-key"]);
    for (field, bad) in [
        ("name", json!("two words")),
        ("name", json!("../escape")),
        ("name", json!("NUL")),
        ("env", json!(["CODETERM_API_TOKEN=override"])),
        ("env", json!(["codetermOther=override"])),
        ("env", json!(["9KEY=value"])),
        ("env", json!(["MODE=one", "mode=two"])),
        ("secretEnv", json!(["TOKEN=raw-value"])),
        ("secretEnv", json!(["TOKEN=domios:provider:other:key"])),
        ("secretEnv", json!(["MODE=domios:provider:acp:key"])),
        ("authEnv", json!(["KEY=domios:provider:acp:key:"])),
        ("authEnv", json!(["KEY=raw-value:api-key"])),
        ("binary", json!("npx")),
    ] {
        let mut value = valid.clone();
        value[field] = bad;
        assert!(request(value).is_err(), "{field}");
    }
}

fn seed_files(id: &str) -> Vec<std::path::PathBuf> {
    let root = installer::profile_root(PROVIDER).unwrap();
    let directory = root.join(id);
    let cwd = directory.join("launch-cwd");
    let install = directory.join("install-fixture");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::create_dir_all(&install).unwrap();
    let binary = install.join("agent");
    let receipt = root.join(format!("{id}.json"));
    let options = root.join(format!("{id}.options.json"));
    for file in [&binary, &receipt, &options] {
        std::fs::write(file, b"fixture").unwrap();
    }
    vec![receipt, options, directory, cwd, binary]
}

fn seed_owner(id: &str) {
    let profile = request(json!({"name":id,"binary":"fixture-agent"}))
        .unwrap()
        .1;
    owner_profiles::update(PROVIDER, |profiles| {
        profiles.insert(id.into(), profile);
        Ok(())
    })
    .unwrap();
}

fn declared(id: &str) -> bool {
    owner_profiles::load(PROVIDER).unwrap().contains_key(id)
}

fn all_exist(files: &[std::path::PathBuf]) -> bool {
    files.iter().all(|file| file.exists())
}

fn none_exist(files: &[std::path::PathBuf]) -> bool {
    files.iter().all(|file| !file.exists())
}

/// `remove` refuses while another removal holds the process-wide mutation lock.
static REMOVALS: parking_lot::Mutex<()> = parking_lot::const_mutex(());

fn fixture() -> (ServiceFixture, String, parking_lot::MutexGuard<'static, ()>) {
    let serial = REMOVALS.lock();
    let (mux, tab) = MultiplexerState::new();
    (ServiceFixture::new(mux), tab, serial)
}

#[test]
fn removal_deletes_the_profiles_own_files_and_no_prefix_sharing_profiles_files() {
    let (fixture, _, _serial) = fixture();
    let svc = &fixture.service;
    seed_owner("remove-own");
    seed_owner("remove-own-extra");
    let own = seed_files("remove-own");
    let longer = seed_files("remove-own-extra");
    let shorter = seed_files("remove-ow");

    remove(svc, "remove-own").unwrap();

    assert!(none_exist(&own), "{own:?}");
    assert!(!declared("remove-own"));
    assert!(all_exist(&longer) && all_exist(&shorter));
    assert!(declared("remove-own-extra"));
    assert_eq!(
        remove(svc, "remove-own").unwrap_err(),
        "Only owner-added profiles can be removed"
    );
}

#[test]
fn shipped_profiles_cannot_be_removed_and_keep_their_files() {
    let (fixture, _, _serial) = fixture();
    let files = seed_files("remove-shipped");
    assert_eq!(
        remove(&fixture.service, "remove-shipped").unwrap_err(),
        "Only owner-added profiles can be removed"
    );
    assert!(all_exist(&files));
}

#[test]
fn a_profile_a_tab_uses_is_refused_with_that_tab_named_and_loses_nothing() {
    let (fixture, tab, _serial) = fixture();
    let svc = &fixture.service;
    seed_owner("remove-live");
    let files = seed_files("remove-live");
    let bind = |agent: &str| {
        let mut mux = svc.mux.write();
        let target = mux.get_tab_mut(&tab).unwrap();
        target.agent_type = Some(agent.into());
        target.agent_runtime_config = Some(json!({"launchProfile":"remove-live"}));
    };

    bind(PROVIDER);
    let refusal = remove(svc, "remove-live").unwrap_err();
    assert!(
        refusal.contains("remove-live") && refusal.contains(&tab),
        "{refusal}"
    );
    assert!(all_exist(&files));
    assert!(declared("remove-live"));

    bind("another-provider");
    remove(svc, "remove-live").unwrap();
    assert!(
        none_exist(&files),
        "another provider's profile is unrelated"
    );
}

#[test]
fn a_profile_named_like_the_owner_store_never_deletes_the_store() {
    let (fixture, _, _serial) = fixture();
    let svc = &fixture.service;
    seed_owner("remove-beside-store");
    seed_owner("owner-profiles");
    let directory = installer::profile_root(PROVIDER)
        .unwrap()
        .join("owner-profiles");
    std::fs::create_dir_all(&directory).unwrap();

    remove(svc, "owner-profiles").unwrap();

    let store = owner_profiles::read(&owner_profiles::path(PROVIDER).unwrap()).unwrap();
    assert!(store.contains_key("remove-beside-store"));
    assert!(!store.contains_key("owner-profiles"));
    assert!(!directory.exists());
    for name in ["owner-profiles", "Owner-Profiles"] {
        assert!(
            available_name(svc, name).unwrap_err().contains("reserved"),
            "{name}"
        );
    }
}

#[test]
fn names_differing_only_by_case_are_refused_because_they_can_share_files() {
    let (fixture, _, _serial) = fixture();
    let svc = &fixture.service;
    seed_owner("remove-case");
    seed_owner("REMOVE-CASE");
    let root = installer::profile_root(PROVIDER).unwrap();
    let receipt = root.join("remove-case.json");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&receipt, b"fixture").unwrap();

    let refusal = remove(svc, "REMOVE-CASE").unwrap_err();

    assert!(refusal.contains("remove-case"), "{refusal}");
    assert!(receipt.exists());
    assert!(declared("remove-case") && declared("REMOVE-CASE"));
}

#[test]
fn a_failed_delete_names_the_path_and_keeps_the_profile_removable() {
    let (fixture, _, _serial) = fixture();
    let svc = &fixture.service;
    seed_owner("remove-retry");
    let root = installer::profile_root(PROVIDER).unwrap();
    std::fs::create_dir_all(&root).unwrap();
    let receipt = root.join("remove-retry.json");
    let blocker = root.join("remove-retry");
    std::fs::write(&receipt, b"fixture").unwrap();
    std::fs::write(&blocker, b"a file where the profile folder belongs").unwrap();

    let refusal = remove(svc, "remove-retry").unwrap_err();

    assert!(refusal.contains("remove-retry"), "{refusal}");
    assert!(declared("remove-retry"));
    assert!(receipt.exists());
    std::fs::remove_file(&blocker).unwrap();
    remove(svc, "remove-retry").unwrap();
    assert!(!receipt.exists() && !declared("remove-retry"));
}

#[test]
fn management_requires_owner_or_live_orchestrator_credentials() {
    let (mut mux, ordinary) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&ordinary).unwrap();
    let orchestrator = mux.create_tab(&workspace).unwrap();
    mux.set_orchestrator_role(&orchestrator).unwrap();
    mux.resolve_or_mint_team_for_parent(&orchestrator).unwrap();
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;
    svc.ext::<crate::config::ConfigState>().lock().api.token = Some("owner-fixture".into());
    let ordinary_token = svc
        .frontend_pty_env(Some(&ordinary))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let orchestrator_token = svc
        .frontend_pty_env(Some(&orchestrator))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    assert!(authorize(svc, "owner-fixture").is_ok());
    assert!(authorize(svc, &orchestrator_token).is_ok());
    assert!(authorize(svc, &ordinary_token).is_err());
    assert!(svc
        .bind_local_rpc(
            &ordinary_token,
            "agent.acp",
            &mut Some(json!({"operation":"list"}))
        )
        .is_err());
    assert!(svc
        .bind_local_rpc(
            &orchestrator_token,
            "agent.acp",
            &mut Some(json!({"operation":"list"}))
        )
        .is_ok());
    assert!(execute(
        svc,
        &json!({"operation":"list","authenticated_mesh_peer_id":"fixture-peer"})
    )
    .is_err());
    assert!(authorize(svc, "forged").is_err());
    svc.revoke_tab_credential(&orchestrator);
    assert!(authorize(svc, &orchestrator_token).is_err());
}
