use super::*;

#[test]
fn startup_diagnostic_never_serializes_error_text_or_request_data() {
    let error = std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "secret callback-url bearer-token request-body",
    );
    let value =
        bootstrap::diagnostic_payload(ClientStartupStage::Install, "io_error", Some(&error), None);
    let encoded = value.to_string();
    assert!(!encoded.contains("secret"));
    assert!(!encoded.contains("callback-url"));
    assert_eq!(value["io_kind"], "PermissionDenied");
    assert_eq!(value["stage"], "install");
    assert!(encoded.len() < 256);
}

#[test]
fn component_manifest_rejects_absolute_and_parent_server_paths() {
    assert!(!bootstrap::valid_server_relative_path(Path::new("/server")));
    assert!(!bootstrap::valid_server_relative_path(Path::new(
        "../server"
    )));
    assert!(bootstrap::valid_server_relative_path(Path::new(
        "bin/domios-server"
    )));
}

#[test]
fn digest_format_requires_exact_lowercase_sha256() {
    assert!(bootstrap::valid_digest(&"a".repeat(64)));
    assert!(!bootstrap::valid_digest(&"A".repeat(64)));
    assert!(!bootstrap::valid_digest(&"a".repeat(63)));
}

#[test]
fn shutdown_cleanup_completion_rejects_protocol_one() {
    assert!(bootstrap::supports_shutdown_completion_protocol(
        codeterm_types::server_bootstrap::BOOTSTRAP_PROTOCOL
    ));
    assert!(!bootstrap::supports_shutdown_completion_protocol(1));
}

#[test]
fn output_flow_queue_full_preserves_the_native_client_and_view() {
    use codeterm_types::native_host::NativeOutputFlow;
    use codeterm_types::server_bootstrap::{
        ClientIdentity, ServerIdentity, API_VERSION, BOOTSTRAP_PROTOCOL, SERVER_COMPONENT_SCHEMA,
    };
    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let process = Arc::new(PinnedProcess::open(std::process::id()).unwrap());
    let provenance = process.provenance().unwrap();
    let root = std::env::temp_dir().join("output-flow-queue-fixture");
    let identity = ServerIdentity {
        data: root.clone(),
        config: root.join("config.toml"),
    };
    let connection = NativeHostConnection {
        instance_id: "fixture".into(),
        identity: identity.clone(),
        server_epoch: "fixture-epoch".into(),
        generation: 1,
    };
    let view = NativeViewBinding::observed("http://127.0.0.1:1420".into()).unwrap();
    let ready = ServerReady {
        protocol: BOOTSTRAP_PROTOCOL,
        api_version: API_VERSION,
        build: "fixture".into(),
        instance_id: "fixture".into(),
        identity,
        epoch: "fixture-epoch".into(),
        process: provenance.clone(),
        api_address: "127.0.0.1:1".parse().unwrap(),
        socket_address: "fixture".into(),
        bootstrap_address: "fixture".into(),
        resources: root.clone(),
    };
    let manifest = ServerComponentManifest {
        schema: SERVER_COMPONENT_SCHEMA,
        server_build: "fixture".into(),
        server_executable: "fixture-server".into(),
        server_sha256: "a".repeat(64),
        client_identity: ClientIdentity::None,
        api_version: API_VERSION,
        bootstrap_protocol: BOOTSTRAP_PROTOCOL,
    };
    let (outbound, receiver) = mpsc::channel(1);
    outbound
        .try_send(Frame::BindView {
            connection: connection.clone(),
            view: view.clone(),
        })
        .unwrap();
    let (shutdown, _) = watch::channel(false);
    let client = Arc::new(NativeHostClient {
        app: app.handle().clone(),
        connection,
        server_process: process,
        ready,
        manifest,
        resource_root: root.clone(),
        server_executable: root.join("fixture-server"),
        client_process: provenance,
        allowed_view_origins: HashSet::from([view.origin.clone()]),
        outbound,
        outbound_rx: Mutex::new(Some(receiver)),
        shutdown,
        retired: AtomicBool::new(false),
        view: Mutex::new(Some(view.clone())),
        root_generation: Mutex::new(None),
        diagnostics: super::super::webview_diagnostics::DiagnosticsSession::new(
            crate::LocalWebviewDiagnosticsMode::Disabled,
        ),
        #[cfg(debug_assertions)]
        publish_dev_locator: false,
        pending: Mutex::new(HashSet::new()),
        request_ids: Mutex::default(),
        cancellations: Mutex::default(),
        last_event_id: AtomicU64::new(0),
    });
    let slot = NativeHostClientSlot(
        Arc::new(RwLock::new(Some(client.clone()))),
        Default::default(),
    );
    let error = slot
        .report_output_flow(Some(&view.origin), NativeOutputFlow::DetachAll)
        .unwrap_err();
    assert!(error.contains("QueueFull"));
    assert!(!client.retired.load(Ordering::Acquire));
    assert!(!*client.shutdown.borrow());
    assert_eq!(client.view.lock().as_ref(), Some(&view));
    assert!(slot.is_current(&client));
}
