use super::*;

#[tokio::test]
async fn process_split_contract_shutdown_reply_follows_successful_cleanup_once() {
    use super::super::channel::after_successful_cleanup;
    use std::cell::Cell;
    let replies = Cell::new(0);
    let result = after_successful_cleanup(Ok(()), || async {
        replies.set(replies.get() + 1);
        Ok(42)
    })
    .await
    .unwrap();
    assert_eq!(result, 42);
    assert_eq!(replies.get(), 1);
}

#[tokio::test]
async fn process_split_contract_failed_cleanup_cannot_send_successful_shutdown_reply() {
    use super::super::channel::after_successful_cleanup;
    use std::cell::Cell;
    let replies = Cell::new(0);
    let result = after_successful_cleanup(
        Err(io::Error::other("fixture persistence refusal")),
        || async {
            replies.set(replies.get() + 1);
            Ok(42)
        },
    )
    .await;
    assert!(result.is_err());
    assert_eq!(replies.get(), 0);
}

#[test]
fn process_split_contract_protocol_one_is_not_cleanup_completion_evidence() {
    let mut old = ready();
    old.protocol = 1;
    let mut request = request(&old);
    assert!(validate_attach(&request, &old).is_err());
    request.protocol = BOOTSTRAP_PROTOCOL;
    assert!(validate_attach(&request, &old).is_err());
    let current = ready();
    request.protocol = 1;
    assert!(validate_attach(&request, &current).is_err());
}

#[test]
fn process_split_contract_only_absent_initial_inspect_authorizes_launch() {
    use super::super::channel::{classify_connect_error, helper_exit_code};
    use codeterm_types::server_bootstrap::{
        ServerExpectation, BOOTSTRAP_ATTACH_UNAVAILABLE_EXIT_CODE,
    };

    let ready = ready();
    let mut request = request(&ready);
    let classify = |request: &AttachRequest, kind| {
        helper_exit_code(&classify_connect_error(
            request,
            &ready.build,
            io::Error::from(kind),
        ))
    };
    for absent in [io::ErrorKind::NotFound, io::ErrorKind::ConnectionRefused] {
        assert_eq!(
            classify(&request, absent),
            BOOTSTRAP_ATTACH_UNAVAILABLE_EXIT_CODE
        );
        assert_eq!(helper_exit_code(&io::Error::from(absent)), 1);
    }
    for failure in [
        io::ErrorKind::PermissionDenied,
        io::ErrorKind::UnexpectedEof,
        io::ErrorKind::InvalidData,
        io::ErrorKind::TimedOut,
        io::ErrorKind::ConnectionReset,
        io::ErrorKind::WouldBlock,
        io::ErrorKind::Other,
    ] {
        assert_eq!(classify(&request, failure), 1);
    }
    request.expected = Some(ServerExpectation {
        process: ready.process.clone(),
        epoch: ready.epoch.clone(),
    });
    assert_eq!(classify(&request, io::ErrorKind::NotFound), 1);
    request.expected = None;
    request.operation = BootstrapOperation::Shutdown;
    assert_eq!(classify(&request, io::ErrorKind::NotFound), 1);
    request.operation = BootstrapOperation::Inspect;
    request.protocol += 1;
    assert_eq!(classify(&request, io::ErrorKind::NotFound), 1);
    request.protocol = ready.protocol;
    request.api_version += 1;
    assert_eq!(classify(&request, io::ErrorKind::NotFound), 1);
    request.api_version = ready.api_version;
    request.build.push_str("-foreign");
    assert_eq!(classify(&request, io::ErrorKind::NotFound), 1);
    request.build = ready.build.clone();
    request.challenge.clear();
    assert_eq!(classify(&request, io::ErrorKind::NotFound), 1);
    assert_eq!(
        helper_exit_code(&io::Error::from(io::ErrorKind::AlreadyExists)),
        3
    );
}

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
fn process_split_contract_observe_preflight_refuses_effects_before_attach_executor() {
    use codeterm_types::server_bootstrap::ServerExpectation;
    use std::cell::Cell;

    let ready = ready();
    let mut request = request(&ready);
    request.expected = Some(ServerExpectation {
        process: ready.process.clone(),
        epoch: ready.epoch.clone(),
    });
    request.operation = BootstrapOperation::Shutdown;
    assert!(validate_attach(&request, &ready).is_ok());
    let executions = Cell::new(0);
    let attach = || {
        executions.set(executions.get() + 1);
        42
    };
    assert!(with_observation_preflight(&request, attach).is_err());
    assert_eq!(executions.get(), 0);

    request.operation = BootstrapOperation::Inspect;
    request.expected = None;
    assert!(with_observation_preflight(&request, attach).is_err());
    assert_eq!(executions.get(), 0);

    request.expected = Some(ServerExpectation {
        process: ready.process.clone(),
        epoch: ready.epoch.clone(),
    });
    request.protocol = 1;
    assert!(with_observation_preflight(&request, attach).is_err());
    assert_eq!(executions.get(), 0);
    request.protocol = BOOTSTRAP_PROTOCOL;
    assert_eq!(with_observation_preflight(&request, attach).unwrap(), 42);
    assert_eq!(executions.get(), 1);
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
        development_owner: None,
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
