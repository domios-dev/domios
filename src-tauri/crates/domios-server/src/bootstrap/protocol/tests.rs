use super::*;

fn ready() -> ServerReady {
    ServerReady {
        protocol: BOOTSTRAP_PROTOCOL,
        api_version: 3,
        build: "test-build".into(),
        instance_id: "test-installation".into(),
        identity: ServerIdentity {
            data: "/isolated".into(),
            config: "/isolated/config.toml".into(),
        },
        epoch: "boot-one".into(),
        process: ProcessProvenance {
            pid: 42,
            created: "creation-one".into(),
            executable: "/server".into(),
        },
        api_address: "127.0.0.1:18886".parse().unwrap(),
        socket_address: "isolated-service".into(),
        bootstrap_address: "isolated-bootstrap".into(),
        resources: "/resources".into(),
    }
}

fn request(ready: &ServerReady) -> AttachRequest {
    AttachRequest {
        protocol: ready.protocol,
        api_version: ready.api_version,
        build: ready.build.clone(),
        identity: ready.identity.clone(),
        challenge: "fresh-challenge".into(),
        operation: BootstrapOperation::Inspect,
        expected: None,
    }
}

#[test]
fn process_split_contract_attach_requires_exact_identity_build_and_protocol() {
    let ready = ready();
    let mut request = request(&ready);
    assert!(validate_attach(&request, &ready).is_ok());
    request.protocol += 1;
    assert!(validate_attach(&request, &ready).is_err());
    request.protocol = ready.protocol;
    request.build.push_str("other");
    assert!(validate_attach(&request, &ready).is_err());
    request.build = ready.build.clone();
    request.identity.config = "/foreign/config.toml".into();
    assert!(validate_attach(&request, &ready).is_err());
}

#[test]
fn process_split_contract_pid_reuse_and_stale_reply_never_authenticate() {
    let ready = ready();
    let request = request(&ready);
    let mut reply = AttachReply {
        challenge: request.challenge.clone(),
        ready: ready.clone(),
        api_token: None,
    };
    assert!(validate_reply(&request, &reply, &ready.process).is_ok());
    reply.ready.process.created = "reused-pid".into();
    assert!(validate_reply(&request, &reply, &ready.process).is_err());
    reply.ready.process = ready.process.clone();
    reply.challenge = "earlier-connection".into();
    assert!(validate_reply(&request, &reply, &ready.process).is_err());
    reply.challenge = request.challenge.clone();
    reply.ready.epoch.clear();
    assert!(validate_reply(&request, &reply, &ready.process).is_err());
}

#[test]
fn process_split_contract_ready_serialization_contains_no_credential() {
    let ready = ready();
    let value = serde_json::to_value(&ready).unwrap();
    assert!(value.get("api_token").is_none());
    assert_eq!(serde_json::from_value::<ServerReady>(value).unwrap(), ready);
}

#[test]
fn process_split_contract_artifact_and_prod_selectors_cannot_cross_attach() {
    use crate::launch::{InstanceSelector, LaunchOptions};
    let mut options = LaunchOptions {
        probe_identity_alias: None,
        instance: InstanceSelector::Development {
            instance: Some("split-oracle".into()),
            port: 7685,
        },
        resources: "/resources".into(),
        probe_sink: None,
    };
    assert!(options.validate(cfg!(debug_assertions)).is_err());
    options.instance = InstanceSelector::Production;
    assert_eq!(
        options.validate(cfg!(debug_assertions)).is_ok(),
        !cfg!(debug_assertions)
    );
    options.instance = InstanceSelector::Development {
        instance: Some("../production".into()),
        port: 17886,
    };
    assert!(options.validate(cfg!(debug_assertions)).is_err());
}
