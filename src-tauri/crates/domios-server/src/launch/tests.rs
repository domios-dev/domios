use super::*;
use codeterm_service_core::ResolvedInstancePaths;
use codeterm_types::server_bootstrap::ServerIdentity;

#[test]
fn process_split_contract_probe_alias_requires_both_roots_and_owned_sink() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("fresh");
    let normal = ResolvedInstancePaths::resolve(&data, &data.join("config.toml")).unwrap();
    let mut options = LaunchOptions {
        instance: InstanceSelector::Development {
            instance: Some("split-oracle-fixture".into()),
            port: 18886,
        },
        resources: root.path().into(),
        probe_sink: Some(data.join("process-init.jsonl")),
        probe_identity_alias: Some(ServerIdentity {
            data: data.join("absent/.."),
            config: data.join("config.toml"),
        }),
    };
    assert_eq!(verify_identity(&options, normal.clone()).unwrap(), normal);
    options.probe_identity_alias.as_mut().unwrap().config = root.path().join("other-config");
    assert!(verify_identity(&options, normal.clone()).is_err());
    options.probe_identity_alias.as_mut().unwrap().config = data.join("config.toml");
    options.probe_sink = Some(root.path().join("foreign-sink"));
    assert!(verify_identity(&options, normal.clone()).is_err());
    options.probe_sink = Some(data.join("process-init.jsonl"));
    options.instance = InstanceSelector::Production;
    assert!(options.validate(false).is_err());
    assert!(verify_identity(&options, normal).is_err());
    assert!(!data.exists());
}

#[test]
fn process_split_contract_ordinary_bootstrap_omits_probe_alias() {
    let options: LaunchOptions = serde_json::from_value(serde_json::json!({
        "instance": { "kind": "development", "instance": "fixture", "port": 18886 },
        "resources": "resources", "probe_sink": null
    }))
    .unwrap();
    assert!(options.probe_identity_alias.is_none());
    assert!(serde_json::to_value(options)
        .unwrap()
        .get("probe_identity_alias")
        .is_none());
}

fn human_options(args: &[&str], development: bool) -> io::Result<HumanStart> {
    let executable = std::env::current_dir()?.join("fixture/domios-server/domios-server");
    human_start_options(
        &args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>(),
        &executable,
        development,
    )
}

#[test]
fn process_split_contract_headless_explicit_selector_and_resource_layout() {
    let production = human_options(&["--production", "--json"], false).unwrap();
    assert!(matches!(
        production.options.instance,
        InstanceSelector::Production
    ));
    assert_eq!(
        production.options.resources,
        std::env::current_dir().unwrap().join("fixture")
    );
    assert!(production.json);
    assert!(production.options.probe_sink.is_none());
    assert!(production.options.probe_identity_alias.is_none());
    let development = human_options(
        &[
            "--development",
            "--port",
            "18886",
            "--instance",
            "fixture",
            "--resources",
            "selected",
        ],
        true,
    )
    .unwrap();
    assert!(
        matches!(development.options.instance, InstanceSelector::Development { instance: Some(name), port: 18886 } if name == "fixture")
    );
    assert_eq!(development.options.resources, PathBuf::from("selected"));
    assert!(!development.json);
    assert!(human_options(&["--development", "--port", "18886"], true).is_ok());
}

#[test]
fn process_split_contract_headless_refuses_ambiguous_or_privileged_options() {
    for args in [
        vec![],
        vec!["--production", "--development"],
        vec!["--production", "--production"],
        vec!["--production", "--port", "18886"],
        vec!["--production", "--instance", "fixture"],
        vec!["--production", "--resources"],
        vec!["--production", "--resources", ""],
        vec!["--production", "--resources", "--json"],
        vec!["--production", "--json", "--json"],
        vec!["--production", "--resources", "one", "--resources", "two"],
        vec!["--production", "--token", "opaque"],
        vec!["--production", "--data", "elsewhere"],
        vec!["--production", "--probe-sink", "elsewhere"],
    ] {
        assert!(human_options(&args, false).is_err(), "{args:?}");
    }
}

#[test]
fn process_split_contract_headless_matches_artifact_and_dev_port_policy() {
    assert!(human_options(&["--production"], true).is_err());
    assert!(human_options(&["--development", "--port", "18886"], false).is_err());
    for args in [
        vec!["--development"],
        vec!["--development", "--port", "0"],
        vec!["--development", "--port", "7685"],
        vec!["--development", "--port", "65536"],
        vec!["--development", "--port", "invalid"],
        vec!["--development", "--port", "18886", "--instance", "../other"],
    ] {
        assert!(human_options(&args, true).is_err(), "{args:?}");
    }
}

#[test]
fn process_split_contract_headless_output_contains_readiness_without_credentials() {
    use crate::bootstrap::protocol::{
        AttachReply, ProcessProvenance, ServerReady, BOOTSTRAP_PROTOCOL,
    };
    let reply = AttachReply {
        challenge: "private-challenge-sentinel".into(),
        api_token: Some("private-token-sentinel".into()),
        ready: ServerReady {
            protocol: BOOTSTRAP_PROTOCOL,
            api_version: codeterm_types::server_bootstrap::API_VERSION,
            build: "fixture-build".into(),
            instance_id: "fixture-instance".into(),
            identity: ServerIdentity {
                data: "fixture/data".into(),
                config: "fixture/config".into(),
            },
            epoch: "fixture-epoch".into(),
            process: ProcessProvenance {
                pid: 42,
                created: "fixture-created".into(),
                executable: "fixture/server".into(),
            },
            api_address: "127.0.0.1:18886".parse().unwrap(),
            socket_address: "fixture/socket".into(),
            bootstrap_address: "fixture/bootstrap".into(),
            resources: "fixture/resources".into(),
        },
    };
    for json in [false, true] {
        let mut output = Vec::new();
        write_human_ready(&mut output, &reply.ready, json).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(!output.contains(reply.api_token.as_ref().unwrap()));
        assert!(!output.contains(&reply.challenge));
        assert!(output.contains(&reply.ready.epoch));
        assert!(output.contains(&reply.ready.api_address.to_string()));
        if json {
            assert_eq!(
                serde_json::from_str::<ServerReady>(&output).unwrap(),
                reply.ready
            );
        }
    }
}

#[cfg(windows)]
#[test]
fn process_split_contract_private_bootstrap_handle_is_not_inherited() {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Foundation::{
            DuplicateHandle, GetHandleInformation, DUPLICATE_SAME_ACCESS, HANDLE_FLAG_INHERIT,
        },
        System::Threading::GetCurrentProcess,
    };
    let source = tempfile::tempfile().unwrap();
    let mut duplicate = std::ptr::null_mut();
    let process = unsafe { GetCurrentProcess() };
    assert_ne!(
        unsafe {
            DuplicateHandle(
                process,
                source.as_raw_handle(),
                process,
                &mut duplicate,
                0,
                1,
                DUPLICATE_SAME_ACCESS,
            )
        },
        0
    );
    let owned = unsafe { OwnedHandle::from_raw_handle(duplicate) };
    let mut before = 0;
    assert_ne!(
        unsafe { GetHandleInformation(owned.as_raw_handle(), &mut before) },
        0
    );
    assert_ne!(before & HANDLE_FLAG_INHERIT, 0);
    exclude_bootstrap_handle_from_children(owned.as_raw_handle()).unwrap();
    let mut after = 0;
    assert_ne!(
        unsafe { GetHandleInformation(owned.as_raw_handle(), &mut after) },
        0
    );
    assert_eq!(after & HANDLE_FLAG_INHERIT, 0);
    assert!(source.metadata().unwrap().is_file());
}
