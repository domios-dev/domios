use super::*;
use std::sync::Arc;

#[test]
fn blueprint_v2_runtime_bundled_probe_recipes_execute() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../resources/blueprints");
    let mut count = 0;
    for file in std::fs::read_dir(root).unwrap() {
        let path = file.unwrap().path();
        if path.extension().and_then(|suffix| suffix.to_str()) != Some("yaml") {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        let document =
            codeterm_blueprint_types::blueprint_document::parse(path.to_str().unwrap(), &source)
                .unwrap()
                .document;
        if document.steps.first().map(|node| node.kind()) != Some("probe") {
            continue;
        }
        count += 1;
        let bindings = document
            .inputs
            .iter()
            .map(|(name, _)| {
                let value = if name == "name" {
                    format!("probe-recipe-{count}")
                } else if name.contains("probe") {
                    "echo ready".into()
                } else if name.starts_with("failed") {
                    r"(?m)^failed\r?$".into()
                } else {
                    r"(?m)^ready\r?$".into()
                };
                format!("{name}={value}")
            })
            .collect::<Vec<_>>();
        let bound = document.bind(&bindings).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let (fixture, storage, handle, lease) = fixture(bound, directory.path());
        crate::scheduler::tick(&handle, &Arc::new(fixture.service.clone()), &lease);
        let current = handle.store.lock().get(&lease.job.id).unwrap().clone();
        let expected = if lease.job.buckets.iter().any(|bucket| bucket == "alldone") {
            "alldone"
        } else {
            "passed"
        };
        assert_eq!(
            current.machine.last_bucket.as_deref(),
            Some(expected),
            "{}: {:?}",
            path.display(),
            current.last_reason
        );
        assert!(current
            .last_reason
            .as_deref()
            .is_some_and(|reason| !reason.starts_with("probe_")));
        assert_eq!(handle.probe_owner.snapshot().unconfirmed, 0);
        drop(handle);
        drop(fixture);
        close(storage);
    }
    assert_eq!(count, 3);
}
