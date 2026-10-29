use super::*;
#[test]
fn cancelled_or_expired_media_lookup_never_enters_the_export() {
    let vm = PluginVm::from_source(
        "lookup-bound",
        r#"module.exports.default={mediaFile(){throw Error("must not enter");}}"#.into(),
    )
    .unwrap();
    let context = ViewCallContext::panel("lookup-bound");
    let cancelled = ConversionCancellation::default();
    cancelled.cancel();
    let error = run(
        &vm,
        "lookup-bound",
        "id",
        &context,
        cancelled,
        Instant::now() + Duration::from_secs(2),
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "view call failed: media lookup cancelled or expired"
    );
    let error = run(
        &vm,
        "lookup-bound",
        "id",
        &context,
        ConversionCancellation::default(),
        Instant::now(),
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "view call failed: media lookup cancelled or expired"
    );
}
#[test]
fn running_media_script_observes_cancellation_and_releases_the_vm() {
    use crate::js_runtime::call_gate;
    let entered = format!("media-running-{}", uuid::Uuid::new_v4().simple());
    let source = format!(
        r#"module.exports.default={{mediaFile(){{host.testGateOpen({});while(true){{}}}},probe(){{return true;}}}}"#,
        serde_json::to_string(&entered).unwrap()
    );
    let vm = Arc::new(PluginVm::from_source("lookup-script", source).unwrap());
    let worker = vm.clone();
    let cancellation = ConversionCancellation::default();
    let stop = cancellation.clone();
    let (done, completed) = std::sync::mpsc::channel();
    let join = std::thread::spawn(move || {
        let result = run(
            &worker,
            "lookup-script",
            "id",
            &ViewCallContext::panel("lookup-script"),
            stop,
            Instant::now() + Duration::from_secs(2),
        );
        done.send(result.is_err()).unwrap();
    });
    let observed = call_gate::wait_opened(&entered, Duration::from_secs(2));
    cancellation.cancel();
    assert!(observed);
    assert!(completed.recv_timeout(Duration::from_secs(2)).unwrap());
    join.join().unwrap();
    assert!(super::super::super::vm::call_json::<bool>(&vm, "probe", &[]).unwrap());
    call_gate::reset(&entered);
}
#[tokio::test]
async fn dropping_the_lookup_future_cancels_the_underlying_awaiting_invocation() {
    use crate::js_runtime::call_gate;
    let entered = format!("media-await-{}", uuid::Uuid::new_v4().simple());
    let release = format!("media-release-{}", uuid::Uuid::new_v4().simple());
    let source = format!(
        r#"module.exports.default={{mediaFile(){{host.testGateOpen({});return host.awaitJob(host.testGateStart({}),function(){{return {{path:"clip.mp4",contentType:"video/mp4"}};}});}}}}"#,
        serde_json::to_string(&entered).unwrap(),
        serde_json::to_string(&release).unwrap()
    );
    let vm = Arc::new(PluginVm::from_source("lookup-drop", source).unwrap());
    let cancellation = ConversionCancellation::default();
    let stop = cancellation.clone();
    let worker = tokio::spawn(lookup(
        vm,
        "lookup-drop".into(),
        "id".into(),
        ViewCallContext::panel("lookup-drop"),
        stop,
        Instant::now() + Duration::from_secs(2),
    ));
    let gate = entered.clone();
    let observed =
        tokio::task::spawn_blocking(move || call_gate::wait_opened(&gate, Duration::from_secs(2)))
            .await
            .unwrap();
    worker.abort();
    let stopped = worker.await;
    call_gate::open(&release);
    assert!(observed);
    assert!(stopped.unwrap_err().is_cancelled());
    assert!(cancellation.is_cancelled());
    call_gate::reset(&entered);
}

fn run_context(plugin: &str) -> crate::media_delivery::RunMediaLookupContext {
    use crate::media_delivery::{RunMediaLookupContext, RunMediaLookupOrigin};
    RunMediaLookupContext {
        origin: RunMediaLookupOrigin::DurableRun,
        plugin: plugin.into(),
        workspace: "workspace".into(),
        team: None,
        owner: "owner".into(),
        run: "run".into(),
        producer_operation: "operation".into(),
        producer_port: "output".into(),
        receiver_node: "receiver".into(),
    }
}
#[test]
fn durable_lookup_refuses_an_ambient_view_scope_even_for_another_plugin_and_restores_it() {
    use crate::view_host::{current_view_call_context, with_view_call_context};
    let _observation = codeterm_decision::physical::RequestScope::observation_only(
        Instant::now() + Duration::from_secs(2),
    );
    let vm = PluginVm::from_source(
        "run-lookup-ambient",
        r#"module.exports.default={mediaFile(){throw Error("must not enter");}}"#.into(),
    )
    .unwrap();
    let ambient = ViewCallContext::panel("another-plugin");
    with_view_call_context("another-plugin", &ambient, || {
        let result = run_durable(
            &vm,
            "run-lookup-ambient",
            "own",
            &run_context("run-lookup-ambient"),
            ConversionCancellation::default(),
            Instant::now() + Duration::from_secs(2),
        );
        assert!(result.is_err());
        assert_eq!(
            current_view_call_context("another-plugin"),
            Some(ambient.clone())
        );
        assert!(!crate::media_delivery::run_media_lookup_active());
        Ok(())
    })
    .unwrap();
    assert!(!crate::view_host::has_view_call_context());
}
#[test]
fn durable_lookup_marker_survives_read_only_sqlite_continuation_and_denies_binary_submission() {
    use crate::binary_transfer::BinaryTransferDescriptor;
    use crate::js_runtime::{host_api::PluginSandbox, host_transfer, manifest::PermissionsConfig};
    let root = tempfile::tempdir().unwrap();
    let database = root.path().join("lookup.sqlite");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .execute_batch("CREATE TABLE observed(value INTEGER); INSERT INTO observed VALUES (1);")
        .unwrap();
    drop(connection);
    let mut permissions = PermissionsConfig::default();
    permissions.network.allow = vec!["api.example".into()];
    let sandbox = Arc::new(
        PluginSandbox::restricted(vec![root.path().into()], vec![])
            .with_identity_and_perms("run-lookup-marker", &permissions)
            .with_binary_transfer(
                Some(BinaryTransferDescriptor::default()),
                root.path(),
                &permissions,
            ),
    );
    let source = format!(
        r#"module.exports.default={{mediaFile(){{const query=JSON.parse(host.sqliteQueryStart({},"SELECT value FROM observed",null));if(typeof query.jobId!=="string")throw Error("missing read query");return host.awaitJob(query.jobId,function(read){{if(!read.rows||read.rows[0].value!==1)throw Error("missing rows");const result=host.transfer.start({{url:"https://api.example/output",method:"GET",response:{{kind:"empty",statuses:[200]}}}});if(result.error!=="not_allowed")throw Error("lookup submitted work");return {{path:"clip.mp4",contentType:"video/mp4"}};}});}}}}"#,
        serde_json::to_string(&database.to_string_lossy()).unwrap()
    );
    let vm = PluginVm::from_source_sandboxed("run-lookup-marker", source, None, sandbox.clone())
        .unwrap();
    let network = Arc::new(NoNetwork(std::sync::atomic::AtomicUsize::new(0)));
    let _session = host_transfer::fixture_session(&sandbox, root.path().into(), network.clone());
    let _observation = codeterm_decision::physical::RequestScope::observation_only(
        Instant::now() + Duration::from_secs(2),
    );
    let result = run_durable(
        &vm,
        "run-lookup-marker",
        "own",
        &run_context("run-lookup-marker"),
        ConversionCancellation::default(),
        Instant::now() + Duration::from_secs(2),
    );
    assert!(result.is_ok());
    assert_eq!(network.0.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(!crate::view_host::has_view_call_context());
    assert!(!crate::media_delivery::run_media_lookup_active());
}

struct NoNetwork(std::sync::atomic::AtomicUsize);
#[async_trait::async_trait]
impl crate::binary_transfer::TestNetwork for NoNetwork {
    async fn request(
        &self,
        _url: &reqwest::Url,
        _method: &str,
        _headers: reqwest::header::HeaderMap,
        _upload: crate::binary_transfer::TestUpload,
    ) -> crate::binary_transfer::Result<crate::binary_transfer::TestResponse> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(crate::binary_transfer::TransferRefusal::TransportUnconfirmed)
    }
}

#[test]
fn durable_lookup_refuses_missing_and_permissive_observers_before_export() {
    struct Permissive;
    impl codeterm_decision::physical::RequestObserver for Permissive {
        fn before_request(&self) -> Result<(), codeterm_decision::DecisionError> {
            Ok(())
        }
    }
    let entered = format!("run-lookup-denied-{}", uuid::Uuid::new_v4().simple());
    let source = format!(
        r#"module.exports.default={{mediaFile(){{host.testGateOpen({});return null;}}}}"#,
        serde_json::to_string(&entered).unwrap()
    );
    let vm = PluginVm::from_source("run-lookup-missing", source).unwrap();
    let context = run_context("run-lookup-missing");
    assert!(codeterm_decision::physical::current().is_none());
    let error = run_durable(
        &vm,
        "run-lookup-missing",
        "own",
        &context,
        ConversionCancellation::default(),
        Instant::now() + Duration::from_secs(2),
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("requires observation-only scope"));
    let _scope = codeterm_decision::physical::RequestScope::enter(Arc::new(Permissive));
    let error = run_durable(
        &vm,
        "run-lookup-missing",
        "own",
        &context,
        ConversionCancellation::default(),
        Instant::now() + Duration::from_secs(2),
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("requires observation-only scope"));
    assert!(!crate::js_runtime::call_gate::is_open(&entered));
    crate::js_runtime::call_gate::reset(&entered);
}

#[tokio::test]
async fn durable_async_lookup_refuses_ambient_view_authority_before_offloading() {
    let vm = Arc::new(
        PluginVm::from_source(
            "run-lookup-before-offload",
            r#"module.exports.default={mediaFile(){throw Error("must not enter");}}"#.into(),
        )
        .unwrap(),
    );
    let result = tokio::task::spawn_blocking(move || {
        let _scope = codeterm_decision::physical::RequestScope::observation_only(
            Instant::now() + Duration::from_secs(2),
        );
        crate::view_host::with_view_call_context(
            "another-plugin",
            &ViewCallContext::panel("another-plugin"),
            || {
                futures_executor::block_on(lookup_run(
                    vm,
                    "run-lookup-before-offload".into(),
                    "own".into(),
                    run_context("run-lookup-before-offload"),
                    ConversionCancellation::default(),
                    Instant::now() + Duration::from_secs(2),
                ))
            },
        )
    })
    .await
    .unwrap();
    assert!(result.is_err());
}
