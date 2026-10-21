use super::*;
use crate::model_providers::EndpointKind;

fn endpoint() -> ModelProviderDefinition {
    ModelProviderDefinition {
        id: "qa-review-endpoint".into(),
        kind: EndpointKind::OpenaiCompatible,
        base_url: "http://127.0.0.1:9/v1".into(),
        models: vec!["qa-route-model".into()],
        key_env: None,
        name: None,
        secret_name: None,
    }
}

#[test]
fn endpoint_models_replace_their_namespace_in_stale_and_future_snapshots() {
    let harness = format!("endpoint-overlay-{}", uuid::Uuid::new_v4());
    let requested = "qa-review-endpoint/qa-route-model";
    let obsolete = "qa-review-endpoint/stale-model";
    super::super::put_live(
        &harness,
        vec![ModelInfo {
            id: obsolete.into(),
            ..Default::default()
        }],
        None,
    );
    put(&harness, vec![endpoint()]);
    assert_eq!(
        super::super::resolve_models(&harness)
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        vec![requested]
    );
    super::super::put_live(
        &harness,
        vec![ModelInfo {
            id: obsolete.into(),
            ..Default::default()
        }],
        None,
    );
    assert!(super::super::resolve_live_catalog_model(&harness, requested).is_some());
    assert!(super::super::resolve_live_catalog_model(&harness, obsolete).is_none());
    assert_eq!(
        resolve(&harness, obsolete).unwrap().unwrap_err(),
        vec![requested]
    );
    put(&harness, Vec::new());
    assert!(resolve(&harness, requested).unwrap().is_err());
    assert!(super::super::resolve_models(&harness).is_empty());
    super::super::unregister(&harness);
}

#[test]
fn configured_endpoints_do_not_require_or_claim_live_harness_discovery() {
    let harness = format!("endpoint-without-live-{}", uuid::Uuid::new_v4());
    super::super::register_catalogue_capability(
        &harness,
        &crate::provider::ModelCatalogueCapability::authoritative_live(),
    );
    put(&harness, vec![endpoint()]);
    assert!(!super::super::has_live_snapshot(&harness));
    assert_eq!(
        super::super::resolve_models(&harness)[0].id,
        "qa-review-endpoint/qa-route-model"
    );
    assert!(resolve(&harness, "qa-review-endpoint/qa-route-model")
        .unwrap()
        .is_ok());
    assert!(resolve(&harness, "QA-REVIEW-ENDPOINT/unknown")
        .unwrap()
        .is_err());
    assert!(resolve(&harness, "another/model").is_none());
    super::super::unregister(&harness);
}
