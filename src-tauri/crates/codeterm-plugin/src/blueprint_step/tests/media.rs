use super::*;
use std::sync::atomic::AtomicBool;

struct Resolver {
    fingerprint: Mutex<String>,
    installed: AtomicBool,
}

impl Resolver {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            fingerprint: Mutex::new("a".repeat(64)),
            installed: AtomicBool::new(true),
        })
    }
}

impl RunMediaResolver for Resolver {
    fn fingerprint(
        &self,
        context: &crate::media_delivery::RunMediaLookupContext,
        media_id: &str,
        kind: PortKind,
    ) -> Result<String, String> {
        if !self.installed.load(Ordering::SeqCst)
            || context.plugin != "fixture"
            || media_id != "image-1"
            || kind != PortKind::Image
        {
            return Err("source unavailable".into());
        }
        Ok(self.fingerprint.lock().clone())
    }
}

#[test]
fn plugin_steps_media_authority_survives_restart_and_refuses_foreign_scope_or_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("media.sqlite");
    let resolver = Resolver::new();
    let context = request("producer").context;
    let handle = {
        let media = RunMediaRegistry::open(&path, resolver.clone()).unwrap();
        let handle = media
            .publish(&context, "fixture", "image", "image-1", PortKind::Image)
            .unwrap();
        assert_eq!(
            media
                .publish(&context, "fixture", "image", "image-1", PortKind::Image)
                .unwrap(),
            handle
        );
        media
            .authorize_handoff(&context, &handle, "reviewer", "review")
            .unwrap();
        handle
    };
    let media = RunMediaRegistry::open(&path, resolver.clone()).unwrap();
    let mut receiving = context.clone();
    receiving.node = "review".into();
    receiving.operation = "review".into();
    let lineage = media
        .input(&receiving, "reviewer", &handle, PortKind::Image)
        .unwrap();
    assert_eq!(
        lineage,
        InputLineage {
            plugin: "fixture".into(),
            operation: "producer".into(),
            port: "image".into()
        }
    );
    assert!(media
        .input(&receiving, "other", &handle, PortKind::Image)
        .is_err());
    for change in 0..4 {
        let mut foreign = receiving.clone();
        match change {
            0 => foreign.scope.workspace = "foreign".into(),
            1 => foreign.scope.team = None,
            2 => foreign.scope.run = "foreign".into(),
            _ => foreign.owner = "foreign".into(),
        }
        assert!(media
            .input(&foreign, "reviewer", &handle, PortKind::Image)
            .is_err());
    }
    *resolver.fingerprint.lock() = "b".repeat(64);
    assert!(media
        .input(&receiving, "reviewer", &handle, PortKind::Image)
        .is_err());
    *resolver.fingerprint.lock() = "a".repeat(64);
    resolver.installed.store(false, Ordering::SeqCst);
    assert!(media
        .input(&receiving, "reviewer", &handle, PortKind::Image)
        .is_err());
}

#[test]
fn plugin_steps_media_outputs_require_host_resolution_and_never_accept_frame_tokens() {
    let request = request("media");
    let ledger = ledger();
    let mut descriptor = descriptor();
    let fixture = Fixture::new();
    descriptor.outputs.get_mut("result").unwrap().kind = PortKind::Image;
    let runtime = runtime(&ledger, &descriptor, fixture.clone());
    assert!(runtime
        .drive(
            "fixture",
            &request,
            Instant::now() + Duration::from_secs(5),
            Default::default()
        )
        .is_err());
    assert_eq!(fixture.submitted.load(Ordering::SeqCst), 0);
    let temp = tempfile::tempdir().unwrap();
    let media = RunMediaRegistry::open(&temp.path().join("media.sqlite"), Resolver::new()).unwrap();
    assert!(media
        .input(
            &request.context,
            "fixture",
            &"a".repeat(64),
            PortKind::Image
        )
        .is_err());
    let runtime = StepRuntime {
        ledger: &ledger,
        descriptor: &descriptor,
        adapter: fixture.clone(),
        media: Some(&media),
    };
    let mut forged = result(OperationState::Succeeded);
    forged.outputs = [(
        "result".into(),
        vec![PortValue::Media {
            handle: "a".repeat(64),
            media: PortKind::Image,
        }],
    )]
    .into();
    *fixture.submit_result.lock() = Ok(forged);
    assert_eq!(drive(&runtime, &request).state, OperationState::Unknown);
}
