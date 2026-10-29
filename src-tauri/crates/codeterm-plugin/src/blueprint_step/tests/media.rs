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
    let ledger = ledger(&request, 10);
    let mut descriptor = descriptor();
    let fixture = Fixture::new(1);
    descriptor.outputs.get_mut("result").unwrap().kind = PortKind::Image;
    let runtime = runtime(&ledger, &descriptor, fixture.clone());
    assert!(runtime
        .drive(
            "fixture",
            &request,
            || 100,
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

#[test]
fn plugin_steps_fixture_media_handoff_review_metadata_and_three_job_history() {
    let generation = request("generate");
    let ledger = ledger(&generation, 10);
    let mut generate_descriptor = descriptor();
    let fixture = Fixture::new(1);
    generate_descriptor.outputs.get_mut("result").unwrap().kind = PortKind::Image;
    let temp = tempfile::tempdir().unwrap();
    let media = RunMediaRegistry::open(&temp.path().join("media.sqlite"), Resolver::new()).unwrap();
    let mut image = result(OperationState::Succeeded);
    image.outputs = [(
        "result".into(),
        vec![PortValue::MediaSource {
            id: "image-1".into(),
            media: PortKind::Image,
        }],
    )]
    .into();
    *fixture.status_result.lock() = Ok(image);
    let generate = StepRuntime {
        ledger: &ledger,
        descriptor: &generate_descriptor,
        adapter: fixture.clone(),
        media: Some(&media),
    };
    drive(&generate, &generation);
    let generated = generate
        .reconcile(
            &generation,
            Instant::now() + Duration::from_secs(5),
            Default::default(),
        )
        .unwrap();
    let PortValue::Media { handle, .. } = &generated.outputs["result"][0] else {
        panic!("host must issue run reference")
    };

    media
        .authorize_handoff(&generation.context, handle, "fixture", "review")
        .unwrap();
    let mut review = request("review");
    review.node_type = "fixture.review".into();
    review.inputs = [(
        "image".into(),
        vec![PortValue::Media {
            handle: handle.clone(),
            media: PortKind::Image,
        }],
    )]
    .into();
    review.context.input_lineage = vec![InputLineage {
        plugin: "fixture".into(),
        operation: "generate".into(),
        port: "result".into(),
    }];
    let mut review_descriptor = descriptor();
    review_descriptor.id = "fixture.review".into();
    review_descriptor.effects.potentially_paid = false;
    review_descriptor.inputs = [(
        "image".into(),
        StepPort {
            kind: PortKind::Image,
            min_items: 1,
            max_items: 1,
            schema: None,
        },
    )]
    .into();
    let reviewer = Fixture::new(0);
    let mut verdict = result(OperationState::Succeeded);
    verdict.job = "job-2".into();
    verdict.outcome = Some("pass".into());
    verdict.outputs = [(
        "result".into(),
        vec![PortValue::Text {
            text: "metadata reference accepted; pixels not inspected".into(),
        }],
    )]
    .into();
    *reviewer.submit_result.lock() = Ok(verdict);
    let review_runtime = StepRuntime {
        ledger: &ledger,
        descriptor: &review_descriptor,
        adapter: reviewer,
        media: Some(&media),
    };
    let reviewed = drive(&review_runtime, &review);
    assert_eq!(reviewed.outcome.as_deref(), Some("pass"));

    let third = request("failed");
    let failure = Fixture::new(1);
    let mut failed = result(OperationState::Failed);
    failed.job = "job-3".into();
    *failure.submit_result.lock() = Ok(failed);
    let plain = descriptor();
    let failing = runtime(&ledger, &plain, failure);
    assert_eq!(drive(&failing, &third).state, OperationState::Failed);
    let waiting = request("waiting");
    let expensive = Fixture::new(9);
    let waiting_runtime = runtime(&ledger, &plain, expensive.clone());
    assert_eq!(
        drive(&waiting_runtime, &waiting).state,
        OperationState::WaitingForOwnerOk
    );
    let history = ledger.history(&generation.context.scope).unwrap();
    assert_eq!(history.len(), 4);
    assert_eq!(history.iter().filter(|job| job.job.is_some()).count(), 3);
    assert_eq!(
        history
            .iter()
            .filter(|job| job.state == OperationState::Failed)
            .count(),
        1
    );
    assert_eq!(
        history[1].request.context.input_lineage,
        review.context.input_lineage
    );
    assert_eq!(
        history[3].waiting.as_ref().unwrap().reason,
        WaitingReason::OverRunCeiling
    );
    assert_eq!(expensive.submitted.load(Ordering::SeqCst), 0);
    waiting_runtime
        .cancel(&waiting, Instant::now() + Duration::from_secs(5))
        .unwrap();
    assert_eq!(
        ledger.get("generate").unwrap().unwrap().outputs,
        generated.outputs
    );
}
