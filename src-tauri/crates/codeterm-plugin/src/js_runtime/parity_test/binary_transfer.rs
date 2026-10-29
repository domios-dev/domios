use crate::binary_transfer::{self as transfer, *};
use crate::js_runtime::{
    call_json,
    host_api::PluginSandbox,
    host_transfer,
    manifest::{CredentialConfig, PermissionsConfig},
    PluginVm,
};
use async_trait::async_trait;
use futures_util::{stream, StreamExt};
use parking_lot::Mutex;
use std::{collections::BTreeMap, sync::Arc};
#[derive(Default)]
struct Network {
    uploads: Mutex<Vec<Vec<u8>>>,
    headers: Mutex<Vec<reqwest::header::HeaderMap>>,
    content_types: Mutex<Vec<Option<String>>>,
    replies: Mutex<std::collections::VecDeque<(u16, String, Vec<u8>)>>,
}
fn bytes() -> Vec<u8> {
    (0..131071).map(|i| (i % 256) as u8).collect()
}
#[async_trait]
impl transfer::TestNetwork for Network {
    async fn request(
        &self,
        _: &reqwest::Url,
        _: &str,
        headers: reqwest::header::HeaderMap,
        mut body: transfer::TestUpload,
    ) -> transfer::Result<transfer::TestResponse> {
        let mut bytes = Vec::new();
        if body.content_type.as_deref() == Some("application/json") {
            assert!(body.length <= MAX_METADATA_BYTES);
        }
        while let Some(chunk) = body.stream.next().await {
            bytes.extend_from_slice(&chunk?);
        }
        self.content_types.lock().push(body.content_type);
        self.uploads.lock().push(bytes);
        self.headers.lock().push(headers);
        let (status, mime, expected) = self
            .replies
            .lock()
            .pop_front()
            .unwrap_or_else(|| (200, "application/octet-stream".into(), self::bytes()));
        let length = expected.len() as u64;
        Ok(transfer::TestResponse {
            status,
            content_type: Some(mime),
            location: None,
            length: Some(length),
            stream: stream::iter(
                expected
                    .chunks(701)
                    .map(|chunk| Ok(chunk.to_vec()))
                    .collect::<Vec<_>>(),
            )
            .boxed(),
            upload: Some(body.state.clone()),
        })
    }
}
#[test]
fn native_and_js_binary_transports_preserve_identical_bytes_and_keep_credentials_host_side() {
    let root = tempfile::tempdir().unwrap();
    let bundle = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("reference.bin"), bytes()).unwrap();
    let credential = root.path().join("auth.json");
    std::fs::write(&credential, r#"{"token":"opaque-secret"}"#).unwrap();
    let mut permissions = PermissionsConfig::default();
    permissions.network.allow = vec!["api.example".into()];
    permissions.secrets = true;
    let credential = CredentialConfig {
        id: "account".into(),
        file: credential.to_string_lossy().into_owned(),
        file_environment: None,
        public: BTreeMap::new(),
        secret: BTreeMap::from([("token".into(), "token".into())]),
    };
    let sandbox = Arc::new(
        PluginSandbox::restricted(vec![root.path().into()], vec![root.path().into()])
            .with_identity_and_perms("parity-binary", &permissions)
            .with_credentials(&[credential])
            .with_binary_transfer(
                Some(BinaryTransferDescriptor {
                    credential_origins: BTreeMap::from([(
                        "declared:account".into(),
                        vec!["https://api.example".into()],
                    )]),
                    ..Default::default()
                }),
                bundle.path(),
                &permissions,
            ),
    );
    let network = Arc::new(Network::default());
    let session = host_transfer::fixture_session(&sandbox, root.path().into(), network.clone());
    let request:TransferRequest=serde_json::from_value(serde_json::json!({"url":"https://api.example/upload","method":"POST","credential":{"kind":"declared","id":"account","headers":{"Authorization":"Bearer {token}"}},"body":{"kind":"file","file":{"kind":"owned","path":"reference.bin","contentType":"application/octet-stream","fileName":"Reference.bin"}},"response":{"kind":"file","statuses":[200],"fileName":"Output.bin","contentType":"application/octet-stream","maxBytes":1024*1024}})).unwrap();
    let native = transfer::execute(session, request.clone(), TransferAbort::default()).unwrap();
    let vm=PluginVm::from_source_sandboxed("parity-binary",r#"module.exports.default={run(request){return host.transfer.async(request,function(result){return result;});}}"#.into(),None,sandbox).unwrap();
    let js: serde_json::Value =
        call_json(&vm, "run", &[serde_json::to_value(&request).unwrap()]).unwrap();
    assert_eq!(js["file"]["size"], native.file().unwrap().size);
    assert_eq!(js["file"]["sha256"], native.file().unwrap().sha256);
    assert_eq!(
        std::fs::read(root.path().join(&native.file().unwrap().path)).unwrap(),
        bytes()
    );
    assert_eq!(
        std::fs::read(root.path().join(js["file"]["path"].as_str().unwrap())).unwrap(),
        bytes()
    );
    assert_eq!(*network.uploads.lock(), vec![bytes(), bytes()]);
    assert!(network
        .headers
        .lock()
        .iter()
        .all(|headers| headers["authorization"] == "Bearer opaque-secret"));
    assert!(!js.to_string().contains("opaque-secret"));
    let mut denied = request;
    denied.body = Some(InputBody::File {
        file: Input::Owned {
            identity: None,
            path: "auth.json".into(),
            content_type: "application/octet-stream".into(),
            file_name: "Source.bin".into(),
        },
    });
    let result: serde_json::Value =
        call_json(&vm, "run", &[serde_json::to_value(denied).unwrap()]).unwrap();
    assert_eq!(result["error"], "not_allowed");
    assert_eq!(network.uploads.lock().len(), 2);
}
#[test]
fn unrestricted_js_still_needs_a_binary_declaration_and_exact_network_grant() {
    let source = r#"module.exports.default={run(request){return host.transfer.async(request,function(result){return result;});}}"#;
    let request = serde_json::json!({"url":"https://api.example/output","method":"GET","response":{"kind":"file","statuses":[200],"fileName":"Output.bin","contentType":"application/octet-stream","maxBytes":100}});
    let vm = PluginVm::from_source("undeclared-transfer", source.into()).unwrap();
    let denied: serde_json::Value = call_json(&vm, "run", &[request.clone()]).unwrap();
    assert_eq!(denied["error"], "not_allowed");
    let root = tempfile::tempdir().unwrap();
    let permissions = PermissionsConfig::default();
    let sandbox = Arc::new(
        PluginSandbox::unrestricted()
            .with_identity_and_perms("ungranted-transfer", &permissions)
            .with_binary_transfer(
                Some(BinaryTransferDescriptor::default()),
                root.path(),
                &permissions,
            ),
    );
    let network = Arc::new(Network::default());
    host_transfer::fixture_session(&sandbox, root.path().into(), network.clone());
    let vm = PluginVm::from_source_sandboxed("ungranted-transfer", source.into(), None, sandbox)
        .unwrap();
    let denied: serde_json::Value = call_json(&vm, "run", &[request]).unwrap();
    assert_eq!(denied["error"], "not_allowed");
    assert!(network.uploads.lock().is_empty());
}

#[test]
fn manifest_binary_transfer_is_typed_and_enforces_host_bounds() {
    use crate::js_runtime::PluginManifest;
    let parse = |descriptor: serde_json::Value| {
        PluginManifest::from_json_str(&serde_json::json!({"id":"fixture","version":"1.0.0","displayName":"Fixture","capabilities":{"binaryTransfer":descriptor}}).to_string())
    };
    assert!(parse(serde_json::json!({})).is_ok());
    for descriptor in [
        serde_json::json!(true),
        serde_json::json!({"maxDownloadBytes":0}),
        serde_json::json!({"timeoutMs":120001}),
        serde_json::json!({"unknown":true}),
        serde_json::json!({"credentialOrigins":{"secret:key":["http://api.example"]}}),
    ] {
        assert!(parse(descriptor).is_err());
    }
}

#[test]
fn native_and_js_preserve_json_and_empty_receipts_without_inventing_files_or_provider_jobs() {
    for empty in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut permissions = PermissionsConfig::default();
        permissions.network.allow = vec!["api.example".into()];
        let plugin = format!("receipt-parity-{}", uuid::Uuid::new_v4().simple());
        let sandbox = Arc::new(
            PluginSandbox::restricted(vec![], vec![])
                .with_identity_and_perms(&plugin, &permissions)
                .with_binary_transfer(
                    Some(BinaryTransferDescriptor::default()),
                    root.path(),
                    &permissions,
                ),
        );
        let (status, mime, bytes) = if empty {
            (204, "", vec![])
        } else {
            (
                202,
                "application/json",
                br#"{"job":"provider-job"}"#.to_vec(),
            )
        };
        let network = Arc::new(Network {
            replies: Mutex::new(
                [
                    (status, mime.into(), bytes.clone()),
                    (status, mime.into(), bytes),
                ]
                .into(),
            ),
            ..Default::default()
        });
        let session = host_transfer::fixture_session(&sandbox, root.path().into(), network);
        let response = if empty {
            serde_json::json!({"kind":"empty","statuses":[204]})
        } else {
            serde_json::json!({"kind":"json","maxBytes":1024,"statuses":[202]})
        };
        let request:TransferRequest=serde_json::from_value(serde_json::json!({"url":"https://api.example/upload","method":"POST","response":response})).unwrap();
        let native = serde_json::to_value(
            transfer::execute(session, request.clone(), TransferAbort::default()).unwrap(),
        )
        .unwrap();
        let vm=PluginVm::from_source_sandboxed(&plugin,r#"module.exports.default={run(request){return host.transfer.async(request,function(result){return result;});}}"#.into(),None,sandbox).unwrap();
        let js: serde_json::Value =
            call_json(&vm, "run", &[serde_json::to_value(request).unwrap()]).unwrap();
        assert_eq!(js, native);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
}
#[test]
fn js_upload_consumes_only_a_fresh_host_selected_origin_bound_file_grant() {
    let root = tempfile::tempdir().unwrap();
    let selected = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(selected.path(), bytes()).unwrap();
    let mut permissions = PermissionsConfig::default();
    permissions.network.allow = vec!["api.example".into()];
    let plugin = format!("selected-parity-{}", uuid::Uuid::new_v4().simple());
    let sandbox = Arc::new(
        PluginSandbox::restricted(vec![], vec![])
            .with_identity_and_perms(&plugin, &permissions)
            .with_binary_transfer(
                Some(BinaryTransferDescriptor::default()),
                root.path(),
                &permissions,
            ),
    );
    let network = Arc::new(Network::default());
    let session = host_transfer::fixture_session(&sandbox, root.path().into(), network.clone());
    let grant = transfer::select_plugin_input(
        &plugin,
        "https://api.example",
        std::fs::File::open(selected.path()).unwrap(),
        "reference.bin",
        "application/octet-stream",
        None,
    )
    .unwrap();
    let vm=PluginVm::from_source_sandboxed(&plugin,r#"module.exports.default={run(request){return host.transfer.async(request,function(result){return result;});}}"#.into(),None,sandbox).unwrap();
    let request = serde_json::json!({"url":"https://api.example/upload","method":"PUT","body":{"kind":"file","file":{"kind":"selected","handle":grant.handle}},"response":{"kind":"file","fileName":"output.bin","contentType":"application/octet-stream","maxBytes":1024*1024,"statuses":[200]}});
    let result: serde_json::Value = call_json(&vm, "run", &[request.clone()]).unwrap();
    assert_eq!(result["kind"], "file");
    assert_eq!(network.uploads.lock().as_slice(), &[bytes()]);
    let denied: serde_json::Value = call_json(&vm, "run", &[request]).unwrap();
    assert_eq!(denied["error"], "not_allowed");
    assert_eq!(network.uploads.lock().len(), 1);
    assert!(transfer::is_current(&session));
}

#[test]
fn in_place_vm_reload_revokes_old_selected_grants_and_rotates_only_its_active_epoch() {
    let root = tempfile::tempdir().unwrap();
    let selected = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(selected.path(), bytes()).unwrap();
    let mut permissions = PermissionsConfig::default();
    permissions.network.allow = vec!["api.example".into()];
    let plugin = format!("reload-transfer-{}", uuid::Uuid::new_v4().simple());
    let sandbox = Arc::new(
        PluginSandbox::restricted(vec![], vec![])
            .with_identity_and_perms(&plugin, &permissions)
            .with_binary_transfer(
                Some(BinaryTransferDescriptor::default()),
                root.path(),
                &permissions,
            ),
    );
    let network = Arc::new(Network::default());
    let old = host_transfer::fixture_session(&sandbox, root.path().into(), network.clone());
    let grant = transfer::select_plugin_input(
        &plugin,
        "https://api.example",
        std::fs::File::open(selected.path()).unwrap(),
        "reference.bin",
        "application/octet-stream",
        None,
    )
    .unwrap();
    let vm = PluginVm::from_source_sandboxed(
        &plugin,
        "module.exports.default={probe(){return true;}}".into(),
        None,
        sandbox.clone(),
    )
    .unwrap();
    vm.reload("module.exports.default={probe(){return false;}}".into())
        .unwrap();
    let fresh = host_transfer::session(&sandbox).unwrap();
    assert_ne!(old.epoch(), fresh.epoch());
    assert!(old.check().is_err());
    assert!(transfer::is_current(&fresh));
    assert!(fresh
        .take_input(
            &grant.handle,
            &reqwest::Url::parse("https://api.example/").unwrap()
        )
        .is_err());
    assert!(network.uploads.lock().is_empty());
}

#[test]
fn native_and_js_json_uploads_preserve_body_and_host_content_type() {
    let root = tempfile::tempdir().unwrap();
    let mut permissions = PermissionsConfig::default();
    permissions.network.allow = vec!["api.example".into()];
    permissions.secrets = true;
    let plugin = format!("json-parity-{}", uuid::Uuid::new_v4().simple());
    let sandbox = Arc::new(
        PluginSandbox::restricted(vec![], vec![])
            .with_identity_and_perms(&plugin, &permissions)
            .with_binary_transfer(
                Some(BinaryTransferDescriptor {
                    credential_origins: BTreeMap::from([(
                        "secret:key".into(),
                        vec!["https://api.example".into()],
                    )]),
                    ..Default::default()
                }),
                root.path(),
                &permissions,
            ),
    );
    let network = Arc::new(Network::default());
    let host: Arc<dyn crate::PluginHostCtx> =
        Arc::new(crate::js_runtime::host_compute::tests::AnalyticsHost {
            secrets: BTreeMap::from([(
                (plugin.clone(), "key".into()),
                "opaque-host-secret".into(),
            )]),
            ..Default::default()
        });
    let vm = PluginVm::from_source_sandboxed(&plugin, "module.exports.default={run(r){return host.transfer.async(r,function(out){return out;});}}".into(), None, sandbox.clone()).unwrap();
    assert!(vm.set_service(Arc::downgrade(&host)));
    let session = host_transfer::fixture_session(&sandbox, root.path().into(), network.clone());
    let request: TransferRequest = serde_json::from_value(serde_json::json!({"url":"https://api.example/generate","method":"POST","headers":{"Content-Type":"text/plain"},"credential":{"kind":"secret","name":"key","header":"xi-api-key"},"body":{"kind":"json","value":{"prompt":"opaque","revision":2}},"response":{"kind":"file","statuses":[200],"fileName":"output.bin","contentType":"application/octet-stream","maxBytes":1024*1024}})).unwrap();
    let native = transfer::execute(session, request.clone(), TransferAbort::default()).unwrap();
    let js: serde_json::Value =
        call_json(&vm, "run", &[serde_json::to_value(&request).unwrap()]).unwrap();
    assert_eq!(js["file"]["sha256"], native.file().unwrap().sha256);
    assert_eq!(
        *network.uploads.lock(),
        vec![br#"{"prompt":"opaque","revision":2}"#.to_vec(); 2]
    );
    assert_eq!(
        *network.content_types.lock(),
        vec![Some("application/json".into()); 2]
    );
    assert!(network
        .headers
        .lock()
        .iter()
        .all(|headers| headers["xi-api-key"] == "opaque-host-secret"
            && !headers.contains_key("content-type")));
    assert!(!js.to_string().contains("opaque-host-secret"));
    let mut oversized = request;
    oversized.body = Some(InputBody::Json {
        value: serde_json::json!({"prompt":"x".repeat(128*1024)}),
    });
    let denied: serde_json::Value =
        call_json(&vm, "run", &[serde_json::to_value(oversized).unwrap()]).unwrap();
    assert!(denied.get("error").is_some());
    assert_eq!(network.uploads.lock().len(), 2);
}
