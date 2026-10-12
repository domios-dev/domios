use super::{
    activation::Activation,
    manifest::{self, Manifest},
    store::Store,
    update::CheckJob,
};
use std::collections::BTreeMap;

const METADATA: &[u8] =
    include_bytes!("../../../scripts/frontend-release/fixtures/frontend-manifest.json");
const SIGNATURE: &str =
    include_str!("../../../scripts/frontend-release/fixtures/frontend-manifest.json.minisig");
const ARCHIVE: &[u8] =
    include_bytes!("../../../scripts/frontend-release/fixtures/frontend-v1.0.0.tar.gz");
const PUBLIC_KEY: &str = "RWRmcm9udGVuZNEdQEUZEh9CtfmYUwGDKXT9+qbRXfdx90/9a+hv/8OA";

fn verified() -> manifest::VerifiedManifest {
    manifest::verify(METADATA, SIGNATURE, ARCHIVE, PUBLIC_KEY, 2, false).unwrap()
}
fn fixture() -> Manifest {
    verified().manifest().clone()
}
fn claims() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("client-a".into(), "token-a".into()),
        ("client-b".into(), "token-b".into()),
    ])
}

#[test]
fn frontend_bundle_compatibility_inclusive_api_range() {
    let mut manifest = fixture();
    assert!(manifest.compatible(2));
    assert!(!manifest.compatible(1));
    assert!(!manifest.compatible(3));
    manifest.min_api_version = 0;
    assert!(!manifest.compatible(2));
    manifest.min_api_version = 3;
    assert!(!manifest.compatible(2));
    let mut fields: serde_json::Value = serde_json::from_slice(METADATA).unwrap();
    fields.as_object_mut().unwrap().remove("minApiVersion");
    assert!(serde_json::from_value::<Manifest>(fields).is_err());
    let mut fields: serde_json::Value = serde_json::from_slice(METADATA).unwrap();
    fields["apiVersion"] = serde_json::json!(2);
    assert!(serde_json::from_value::<Manifest>(fields).is_err());
}

#[test]
fn frontend_bundle_integrity_accepts_producer_prehashed_minisign_fixture() {
    assert_eq!(fixture().canonical_bytes().unwrap(), METADATA);
    assert_eq!(fixture().release_id, "frontend-v1.0.0");
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    assert!(store
        .install(&verified(), SIGNATURE, b"changed after verification")
        .is_err());
    store.install(&verified(), SIGNATURE, ARCHIVE).unwrap();
    assert!(store
        .asset(&fixture().release_id, "index.html")
        .unwrap()
        .starts_with(b"<!doctype"));
    store
        .verify_release(&fixture(), PUBLIC_KEY, 2, false)
        .unwrap();
}

#[test]
fn frontend_bundle_integrity_metadata_and_digest_are_bound() {
    for (field, value) in [
        ("version", "1.0.1"),
        ("url", "https://example.test/evil.tar.gz"),
        ("releaseId", "evil"),
        (
            "sha256",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ),
    ] {
        let mut metadata: serde_json::Value = serde_json::from_slice(METADATA).unwrap();
        metadata[field] = serde_json::json!(value);
        assert!(manifest::verify(
            &serde_json::to_vec(&metadata).unwrap(),
            SIGNATURE,
            ARCHIVE,
            PUBLIC_KEY,
            2,
            false
        )
        .is_err());
    }
    for field in ["minApiVersion", "maxApiVersion"] {
        let mut metadata: serde_json::Value = serde_json::from_slice(METADATA).unwrap();
        metadata[field] = serde_json::json!(1);
        assert!(manifest::verify(
            &serde_json::to_vec(&metadata).unwrap(),
            SIGNATURE,
            ARCHIVE,
            PUBLIC_KEY,
            2,
            false
        )
        .is_err());
    }
    let mut corrupted = ARCHIVE.to_vec();
    corrupted[20] ^= 1;
    assert!(manifest::verify(METADATA, SIGNATURE, &corrupted, PUBLIC_KEY, 2, false).is_err());
    assert!(manifest::verify(METADATA, "bad signature", ARCHIVE, PUBLIC_KEY, 2, false).is_err());
    assert!(manifest::verify(METADATA, SIGNATURE, ARCHIVE, "bad public key", 2, false).is_err());
}

#[test]
fn frontend_bundle_integrity_traversal_and_truncated_archives() {
    for path in [
        "../outside",
        "/absolute",
        "a/../../outside",
        "a\\evil",
        "C:evil",
        "NUL",
        "con.txt",
        "a/./b",
        "a//b",
        "a.",
    ] {
        assert!(!super::store::safe_asset_path(path), "{path}");
    }
    let root = tempfile::tempdir().unwrap();
    for archive in [b"broken".as_slice(), &ARCHIVE[..ARCHIVE.len() - 1]] {
        assert!(super::store::extract(archive, root.path()).is_err());
    }
}

#[test]
fn frontend_bundle_activation_requires_every_exact_health_claim() {
    let mut state = Activation::default();
    state.stage(&verified()).unwrap();
    let release = fixture().release_id;
    assert_eq!(state.begin(&release, claims(), 100, 2).unwrap(), 1);
    assert!(state.active.is_none());
    assert!(state.begin(&release, claims(), 100, 2).is_err());
    assert!(state.healthy(&release, 0, "client-a", "token-a").is_err());
    assert!(state.healthy("foreign", 1, "client-a", "token-a").is_err());
    assert!(state.healthy(&release, 1, "foreign", "token-a").is_err());
    assert!(state.healthy(&release, 1, "client-a", "token-b").is_err());
    assert!(!state.healthy(&release, 1, "client-a", "token-a").unwrap());
    assert!(state.active.is_none());
    assert!(state.healthy(&release, 1, "client-b", "token-b").unwrap());
    assert_eq!(state.active, Some(fixture()));
    assert!(!state.expire(100));
    assert!(state.quarantined.is_empty());
    assert!(state.healthy(&release, 1, "client-a", "token-a").unwrap());
    assert!(state.healthy(&release, 1, "client-a", "foreign").is_err());
}

#[test]
fn frontend_bundle_activation_deadline_quarantines_without_success() {
    let mut state = Activation::default();
    state.stage(&verified()).unwrap();
    state
        .begin(&fixture().release_id, claims(), 100, 2)
        .unwrap();
    assert!(!state.expire(99));
    assert!(state.expire(100));
    assert!(state.active.is_none());
    assert!(state.pending.is_none());
    assert!(state.quarantined.contains(&fixture().release_id));
    assert!(state.stage(&verified()).is_err());
    assert!(!state.expire(101));
}

#[test]
fn frontend_bundle_partial_health_deadline_preserves_active_and_quarantines_candidate() {
    let mut old = fixture();
    old.release_id = "previous".into();
    let mut state = Activation {
        active: Some(old.clone()),
        ..Activation::default()
    };
    state.stage(&verified()).unwrap();
    let release = fixture().release_id;
    state.begin(&release, claims(), 100, 2).unwrap();
    assert!(!state.healthy(&release, 1, "client-a", "token-a").unwrap());
    assert!(state.expire(100));
    assert_eq!(state.active, Some(old));
    assert!(state.pending.is_none());
    assert!(state.quarantined.contains(&release));
    assert!(state.healthy(&release, 1, "client-b", "token-b").is_err());
}

#[test]
fn frontend_bundle_activation_crash_recovers_previous_verified_state() {
    let root = tempfile::tempdir().unwrap();
    let mut store = Store::open(root.path()).unwrap();
    let mut old = fixture();
    old.release_id = "previous".into();
    let mut state = Activation {
        active: Some(old.clone()),
        ..Activation::default()
    };
    store.save(&state).unwrap();
    state.stage(&verified()).unwrap();
    store.save(&state).unwrap();
    state
        .begin(&fixture().release_id, claims(), 100, 2)
        .unwrap();
    store.save(&state).unwrap();
    drop(store);
    let store = Store::open(root.path()).unwrap();
    let mut recovered = store.load().unwrap();
    recovered.recover(2);
    assert_eq!(recovered.active, Some(old));
    assert!(recovered.loading.is_none());
    assert!(recovered.pending.is_none());
    assert!(recovered.quarantined.contains(&fixture().release_id));
}

#[test]
fn frontend_bundle_activation_rollback_chain_and_stale_generation() {
    let mut old = fixture();
    old.release_id = "previous".into();
    let mut state = Activation {
        active: Some(old.clone()),
        ..Activation::default()
    };
    state.stage(&verified()).unwrap();
    state
        .begin(
            &fixture().release_id,
            BTreeMap::from([("client-a".into(), "token-a".into())]),
            100,
            2,
        )
        .unwrap();
    state
        .healthy(&fixture().release_id, 1, "client-a", "token-a")
        .unwrap();
    assert!(state.rollback_active(&fixture().release_id, 0).is_err());
    state.rollback_active(&fixture().release_id, 1).unwrap();
    assert_eq!(state.active, Some(old));
    state.rollback_active("previous", 1).unwrap();
    assert!(state.active.is_none());
    assert!(state.stage(&verified()).is_err());
}

#[test]
fn frontend_bundle_activation_single_flight_releases_on_failure() {
    let job = CheckJob::default();
    let lease = job.acquire().unwrap();
    assert!(job.acquire().is_err());
    drop(lease);
    assert!(job.acquire().is_ok());
}

#[test]
fn frontend_bundle_routes_contract_checkpoint_scope_revision_and_persistence() {
    let root = tempfile::tempdir().unwrap();
    let mut store = Store::open(root.path()).unwrap();
    let payload = serde_json::json!({"drafts": {"tab": {"text": "exact final edit", "attachmentIds": ["a", "b"]}}, "scroll": {"tab": "anchor"}});
    store.put_checkpoint("client-a", 7, &payload).unwrap();
    assert!(store.put_checkpoint("client-a", 6, &payload).is_err());
    assert!(store
        .put_checkpoint("client-a", 7, &serde_json::json!({}))
        .is_err());
    store.put_checkpoint("client-a", 7, &payload).unwrap();
    store
        .put_checkpoint("client-b", 1, &serde_json::json!({"other": true}))
        .unwrap();
    drop(store);
    let store = Store::open(root.path()).unwrap();
    assert_eq!(store.checkpoint("client-a").unwrap(), Some((7, payload)));
    assert_eq!(
        store.checkpoint("client-b").unwrap(),
        Some((1, serde_json::json!({"other": true})))
    );
}

#[tokio::test]
async fn frontend_bundle_routes_contract_real_handlers_and_authority() {
    use crate::api::frontend_bundle_routes::{routes, FrontendRouteState};
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use std::sync::Arc;
    use tower::ServiceExt;
    let root = tempfile::tempdir().unwrap();
    let bundles = Arc::new(
        super::FrontendBundles::open(
            root.path(),
            2,
            "0.0.0",
            None,
            "https://example.test/manifest.json".into(),
            false,
            Arc::new(|_, _| {}),
        )
        .unwrap(),
    );
    let app = routes(FrontendRouteState {
        bundles,
        is_owner: Arc::new(|headers| {
            headers
                .get("authorization")
                .is_some_and(|value| value == "Bearer owner")
        }),
    });
    let body = serde_json::json!({"revision": 1, "payload": {"drafts": {"t": {"text": "last edit", "attachmentIds": ["a"]}}, "scroll": {"t": "anchor"}}}).to_string();
    for authority in [None, Some("Bearer peer"), Some("Bearer push")] {
        let mut request = Request::builder()
            .method("PUT")
            .uri("/api/frontend/clients/client-a/checkpoint")
            .header("content-type", "application/json");
        if let Some(authority) = authority {
            request = request.header("authorization", authority);
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::from(body.clone())).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/frontend/clients/client-a/checkpoint")
                .header("authorization", "Bearer owner")
                .header("content-type", "application/json")
                .body(Body::from(body.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let saved: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert_eq!(
        saved,
        serde_json::from_str::<serde_json::Value>(&body).unwrap()
    );
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/frontend/clients/client-a/checkpoint")
                .header("authorization", "Bearer owner")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let restored: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert_eq!(restored, saved);
    for (path, body) in [
        ("/api/frontend/check", "{}"),
        (
            "/api/frontend/activate",
            r#"{"releaseId":"frontend-v1.0.0","clients":["client-a"]}"#,
        ),
        (
            "/api/frontend/healthy",
            r#"{"releaseId":"frontend-v1.0.0","generation":1,"clientId":"client-a","token":"foreign"}"#,
        ),
        (
            "/api/frontend/rollback",
            r#"{"releaseId":"frontend-v1.0.0","generation":1}"#,
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header("authorization", "Bearer owner")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT, "{path}");
    }
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/frontend/status")
                .header("authorization", "Bearer owner")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let status: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert_eq!(status["apiVersion"], 2);
    assert_eq!(status["updatesEnabled"], false);
    assert!(status.get("claims").is_none());
}

#[test]
fn frontend_bundle_integrity_recovery_rejects_corrupt_extracted_assets() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    store.install(&verified(), SIGNATURE, ARCHIVE).unwrap();
    std::fs::write(
        root.path()
            .join("releases/frontend-v1.0.0/assets-root/index.html"),
        b"tampered",
    )
    .unwrap();
    assert!(store
        .verify_release(&fixture(), PUBLIC_KEY, 2, false)
        .is_err());
    assert!(store.install(&verified(), SIGNATURE, ARCHIVE).is_err());
}

#[test]
fn frontend_bundle_activation_failed_transaction_keeps_all_pointers() {
    let root = tempfile::tempdir().unwrap();
    let mut store = Store::open(root.path()).unwrap();
    let initial = Activation::default();
    store.save(&initial).unwrap();
    let db = rusqlite::Connection::open(root.path().join("frontend.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_pointer_write BEFORE UPDATE ON activation BEGIN SELECT RAISE(ABORT, 'injected commit failure'); END;").unwrap();
    let mut candidate = initial.clone();
    candidate.stage(&verified()).unwrap();
    assert!(store.save(&candidate).is_err());
    drop(store);
    assert_eq!(Store::open(root.path()).unwrap().load().unwrap(), initial);
}

#[tokio::test]
async fn frontend_bundle_routes_contract_verified_activation_assets_health_and_rollback() {
    use crate::api::frontend_bundle_routes::{routes, FrontendRouteState};
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use std::sync::Arc;
    use tower::ServiceExt;
    let root = tempfile::tempdir().unwrap();
    let mut store = Store::open(root.path()).unwrap();
    store.install(&verified(), SIGNATURE, ARCHIVE).unwrap();
    let mut state = Activation::default();
    state.stage(&verified()).unwrap();
    store.save(&state).unwrap();
    drop(store);
    let bundles = Arc::new(
        super::FrontendBundles::open(
            root.path(),
            2,
            "0.0.0",
            Some(PUBLIC_KEY.into()),
            "https://example.test/manifest.json".into(),
            false,
            Arc::new(|_, _| {}),
        )
        .unwrap(),
    );
    let app = routes(FrontendRouteState {
        bundles,
        is_owner: Arc::new(|headers| {
            headers
                .get("authorization")
                .is_some_and(|value| value == "Bearer owner")
        }),
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/frontend/activate")
                .header("authorization", "Bearer owner")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"releaseId":"frontend-v1.0.0","clients":["client-a"]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let activation: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 16384).await.unwrap()).unwrap();
    assert_eq!(activation["releaseId"], "frontend-v1.0.0");
    let healthy = serde_json::json!({"releaseId": activation["releaseId"], "generation": activation["generation"], "clientId": "client-a", "token": activation["claims"]["client-a"]});
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/frontend/healthy")
                .header("authorization", "Bearer owner")
                .header("content-type", "application/json")
                .body(Body::from(healthy.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let status: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 16384).await.unwrap()).unwrap();
    assert_eq!(status["active"]["releaseId"], "frontend-v1.0.0");
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/frontend/frontend-v1.0.0/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "text/html; charset=utf-8"
    );
    let html = String::from_utf8(
        to_bytes(response.into_body(), 16384)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(html.find("/frontend/bootstrap.js").unwrap() < html.find("assets/fixture.js").unwrap());
    let response = app.clone().oneshot(Request::builder().method("POST").uri("/api/frontend/rollback").header("authorization", "Bearer owner").header("content-type", "application/json").body(Body::from(serde_json::json!({"releaseId": activation["releaseId"], "generation": activation["generation"]}).to_string())).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let status: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 16384).await.unwrap()).unwrap();
    assert!(status["active"].is_null());
    assert_eq!(status["quarantined"][0], "frontend-v1.0.0");
}

#[test]
fn frontend_bundle_activation_duplicate_checks_do_not_request_repeated_swaps() {
    let mut state = Activation::default();
    assert!(state.stage(&verified()).unwrap());
    assert!(!state.stage(&verified()).unwrap());
    state
        .begin(
            &fixture().release_id,
            BTreeMap::from([("client-a".into(), "token-a".into())]),
            100,
            2,
        )
        .unwrap();
    state
        .healthy(&fixture().release_id, 1, "client-a", "token-a")
        .unwrap();
    assert!(!state.stage(&verified()).unwrap());
    assert!(state.pending.is_none());
}

#[test]
fn only_a_strictly_newer_release_passes_the_version_floor() {
    let floor = semver::Version::parse("1.14.2").unwrap();
    assert!(manifest::newer_than("1.14.3", &floor));
    assert!(!manifest::newer_than("1.14.2", &floor));
    assert!(!manifest::newer_than("1.14.1", &floor));
    assert!(!manifest::newer_than("not-a-version", &floor));
}

#[test]
fn stored_releases_not_newer_than_the_app_frontend_are_retired() {
    let mut state = Activation {
        active: Some(fixture()),
        pending: Some(fixture()),
        ..Activation::default()
    };
    state.retire_not_newer(&semver::Version::parse("0.9.0").unwrap());
    assert_eq!(state.active, Some(fixture()));
    state.retire_not_newer(&semver::Version::parse("1.0.0").unwrap());
    assert_eq!(state.active, None);
    assert_eq!(state.pending, None);
}

#[test]
fn bundled_frontend_version_matches_the_release_contract() {
    let contract: serde_json::Value =
        serde_json::from_str(include_str!("../../../frontend-release-contract.json")).unwrap();
    assert_eq!(
        contract["version"].as_str(),
        Some(crate::frontend_bundle_api_version::BUNDLED_FRONTEND_VERSION)
    );
}
