use super::*;
use crate::blueprint_document::{calls, parse};

fn fixtures() -> Vec<Value> {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tests/fixtures/blueprint-identities-1.15.2.golden.json"
    )))
    .unwrap()
}

#[test]
fn blueprint_stored_identity_1152_golden_checks_content_under_both_layouts() {
    for fixture in fixtures() {
        let document: Document = serde_json::from_value(fixture["document"].clone()).unwrap();
        let recorded = fixture["hash"].as_str().unwrap();
        assert_ne!(identity(&document).unwrap(), recorded);
        assert!(
            matches_identity(&document, recorded).unwrap(),
            "{}",
            fixture["name"]
        );
        assert!(matches_identity(&document, &identity(&document).unwrap()).unwrap());
        let serialized = serde_json::to_string(&document).unwrap();
        let roundtrip: Document = serde_json::from_str(&serialized).unwrap();
        assert!(matches_identity(&roundtrip, recorded).unwrap());
        let mut changed = document.clone();
        changed.note = Some("changed bytes".into());
        assert!(!matches_identity(&changed, recorded).unwrap());
        let mut changed = document;
        changed.steps[0].session = !changed.steps[0].session;
        assert!(!matches_identity(&changed, recorded).unwrap());
        assert!(!matches_identity(&changed, "unrelated recorded hash").unwrap());
    }
}

struct Recorded(calls::FrozenCallee);
impl calls::MemoryResolver for Recorded {
    fn head(&self, _: &str) -> Result<Document, String> {
        Ok((*self.0.document).clone())
    }
    fn revision(&self, pin: &calls::CallPin) -> Result<Document, String> {
        if pin != &self.0.pin() {
            return Err("Missing immutable revision".into());
        }
        Ok((*self.0.document).clone())
    }
}

#[test]
fn blueprint_stored_identity_1152_closure_import_export_and_drift_keep_recorded_pin() {
    let fixture = fixtures()
        .into_iter()
        .find(|fixture| fixture["name"] == "call")
        .unwrap();
    let document: Document = serde_json::from_value(fixture["document"].clone()).unwrap();
    let closure = calls::FrozenClosure::from_document(document.clone()).unwrap();
    let callee = closure.callees.values().next().unwrap().clone();
    let recorded = callee.pin();
    callee.verify().unwrap();
    calls::projection::graph(&closure).unwrap();
    let yaml = document.yaml().unwrap();
    let exported = parse("export.yaml", &yaml).unwrap().document;
    let bundle = calls::collect_local("export.yaml", &yaml).unwrap();
    assert_eq!(bundle.document, exported);
    assert!(bundle.local_callees.is_empty());
    let memory = Recorded(callee);
    let imported = calls::resolve_bundle(bundle.document, &[], &memory).unwrap();
    assert_eq!(
        imported.root.document.steps[0]
            .call
            .as_ref()
            .unwrap()
            .pin
            .as_ref(),
        Some(&recorded)
    );
    assert_eq!(imported.callee(&recorded).unwrap().pin(), recorded);
    assert!(imported.drift(&memory).unwrap().is_empty());
    let mut tampered = memory.0.clone();
    tampered.document.steps[0].run = Some(crate::blueprint_document::ValueForm::literal(
        "echo changed",
    ));
    assert!(tampered.verify().is_err());
    assert!(imported.drift(&Recorded(tampered)).is_err());
    let mut tampered_root = document;
    tampered_root
        .dependencies
        .values_mut()
        .next()
        .unwrap()
        .document
        .note = Some("tampered".into());
    assert!(calls::FrozenClosure::from_document(tampered_root).is_err());
}
