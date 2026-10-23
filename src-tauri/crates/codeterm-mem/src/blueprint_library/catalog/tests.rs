use super::*;
use codeterm_blueprint_types::blueprint_document::{checker, projection};

#[test]
fn blueprint_v2_intent_catalog_is_native_typed_and_has_no_reference_runtime() {
    assert_eq!(ENTRIES.len(), 6);
    for entry in ENTRIES {
        let document = document(entry.id).unwrap();
        assert!(document.trigger.is_none(), "{}", entry.id);
        assert!(document.result.port.is_some(), "{}", entry.id);
        assert!(document.result.from.is_some(), "{}", entry.id);
        assert!(document.imports.is_empty(), "{}", entry.id);
        assert!(document.nodes().all(|n| n.kind() != "input"));
        assert!(
            document.steps[0]
                .input
                .as_ref()
                .is_none_or(|port| port.reference.is_none()),
            "{}: initial call arguments have no schema_id",
            entry.id
        );
        let diagnostics = checker::check_document(&document, &checker::CheckContext::default());
        assert!(
            !checker::has_errors(&diagnostics),
            "{}: {diagnostics:?}",
            entry.id
        );
        assert_eq!(projection::identity(&document).unwrap().len(), 64);
        assert!(!entry.source.contains("bpl "));
    }
}

#[test]
fn blueprint_v2_intent_catalog_gaps_are_explicit_and_cannot_open_a_body() {
    assert_eq!(UNAVAILABLE.len(), 10);
    for (id, reason) in UNAVAILABLE {
        assert!(document(id).unwrap_err().contains(reason));
    }
    assert!(document("catalog:missing").is_err());
    assert!(document("severity-grade").is_err());
}
