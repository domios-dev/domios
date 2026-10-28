use super::*;

mod neighbor_authority;
mod neighbor_files;
mod neighbor_read;

#[tokio::test]
async fn recipient_pause_returns_a_typed_conflict_without_limit_metadata() {
    let response =
        neighbor_failure("recipient_paused: This Neighbor paused messages from you".to_string());
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["reason"], "recipient_paused");
    assert_eq!(body["code"], "recipient_paused");
    assert!(body.get("limit").is_none());
}

#[tokio::test]
async fn aggregate_daily_send_limit_returns_429_with_limit_and_reset() {
    let response = neighbor_failure(
        "aggregate_daily_limited: {\"limit\":500,\"resets_at\":1793145600}".into(),
    );
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["reason"], "aggregate_daily_limited");
    assert_eq!(body["code"], "aggregate_daily_limited");
    assert_eq!(body["limit"], 500);
    assert_eq!(body["resets_at"], 1793145600u64);
}

#[tokio::test]
async fn size_and_permission_refusals_keep_their_typed_reason_and_limit() {
    let response = neighbor_failure("message_too_large: {\"limit\":32752}".into());
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["reason"], "message_too_large");
    assert_eq!(body["limit"], 32752);
    for reason in [
        "tab_requests_not_allowed",
        "neighbor_permission_denied",
        "neighbor_paused",
        "neighbor_memory_not_allowed",
        "neighbor_memory_read_only",
        "neighbor_secrets_not_allowed",
        "neighbor_files_not_allowed",
    ] {
        let response = neighbor_failure(format!("{reason}: This request is not allowed"));
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()["reason"],
            reason
        );
    }
}

#[tokio::test]
async fn ended_neighbor_history_retains_a_typed_conflict_and_diagnostic() {
    let response = neighbor_failure("friendship_ended: This Neighbor conversation has ended; refresh the friendship before sending".into());
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["reason"], "friendship_ended");
    assert!(body["message"]
        .as_str()
        .unwrap()
        .contains("conversation has ended"));
}

#[tokio::test]
async fn neighbor_limit_errors_keep_typed_status_and_fields() {
    let response =
        neighbor_failure("neighbor_daily_limit: {\"limit\":5,\"resets_at\":1000}".into());
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["reason"], "neighbor_daily_limit");
    assert_eq!(body["limit"], 5);
    assert_eq!(body["resets_at"], 1000);
    for reason in ["rate_limited", "aggregate_limited"] {
        let response = neighbor_failure(format!("{reason}: {{\"retry_after_seconds\":42}}"));
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["reason"], reason);
        assert_eq!(body["retry_after_seconds"], 42);
    }
    for reason in ["friends_locked", "recipient_locked"] {
        assert_eq!(
            neighbor_failure(format!("{reason}: plan unavailable")).status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        neighbor_failure("limit_out_of_range: invalid daily limit".into()).status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn neighbor_disabled_response_keeps_a_typed_reason() {
    let response =
        neighbor_failure("neighbors_disabled: Neighbors is turned off on this instance".into());
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["code"], "neighbors_disabled");
    assert_eq!(body["reason"], "neighbors_disabled");
}

#[tokio::test]
async fn pending_login_route_is_compiled_out_without_debug_assertions() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(
        codeterm_service_core::multiplexer::state::MultiplexerState::empty(),
    );
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    let response = debug_login_router()
        .with_state(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/account/login/pending")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        if cfg!(debug_assertions) {
            StatusCode::METHOD_NOT_ALLOWED
        } else {
            StatusCode::NOT_FOUND
        }
    );
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn pending_login_route_requires_local_auth_and_returns_404_when_idle() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(
        codeterm_service_core::multiplexer::state::MultiplexerState::empty(),
    );
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    state.config.lock().api.token = Some("fixture-token".into());
    state.config.lock().account = serde_json::json!({"provider":"local"});
    let router = crate::routes::create_router(state);
    for (token, expected) in [
        ("", StatusCode::UNAUTHORIZED),
        ("Bearer fixture-token", StatusCode::NOT_FOUND),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/account/login/pending")
                    .header("authorization", token)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
}

#[test]
fn account_routes_admission_refuses_nonlocal_callers() {
    assert_eq!(admit(None).unwrap_err().0, StatusCode::FORBIDDEN);
    assert!(admit(Some(&Extension(CallerIdentity::Local))).is_ok());
}

#[tokio::test]
async fn gift_redemption_route_requires_auth_and_redacts_malformed_payloads() {
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use tower::ServiceExt;
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(
        codeterm_service_core::multiplexer::state::MultiplexerState::empty(),
    );
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    state.config.lock().api.token = Some("fixture-token".into());
    let router = crate::routes::create_router(state);
    for (token, body, expected) in [
        ("", "{}".to_owned(), StatusCode::UNAUTHORIZED),
        (
            "Bearer fixture-token",
            "{private-material".to_owned(),
            StatusCode::BAD_REQUEST,
        ),
        (
            "Bearer fixture-token",
            r#"{"key":"private-material","expected_uid":"own","plan":"admin"}"#.to_owned(),
            StatusCode::BAD_REQUEST,
        ),
        (
            "Bearer fixture-token",
            serde_json::json!({"key":"private-material".repeat(100),"expected_uid":"own"})
                .to_string(),
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/account/gift/redeem")
                    .header("authorization", token)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("private-material"));
        assert!(fixture
            .service
            .ext::<codeterm_service_core::service::account::AccountCell>()
            .0
            .get()
            .is_none());
    }
}

#[tokio::test]
async fn account_routes_local_journey_persists_and_deletes_credential() {
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use tower::ServiceExt;
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(
        codeterm_service_core::multiplexer::state::MultiplexerState::empty(),
    );
    *fixture.service.secrets.write().await =
        std::sync::Arc::new(codeterm_service_core::secrets::mock::MockStore::new());
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    state.config.lock().api.token = Some("fixture-token".into());
    state.config.lock().account = serde_json::json!({"provider":"local"});
    let router = crate::routes::create_router(state);
    let store = fixture.service.secrets.read().await.clone();
    for (method, path, signed_in) in [
        ("GET", "/api/account/status", false),
        (
            "POST",
            "/api/account/login?provider=email&mode=signup",
            true,
        ),
        ("GET", "/api/account/status", true),
        ("POST", "/api/account/logout", false),
        ("GET", "/api/account/status", false),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("authorization", "Bearer fixture-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        assert_eq!(
            status,
            StatusCode::OK,
            "{method} {path}: {}",
            String::from_utf8_lossy(&body)
        );
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["identity"].is_object(), signed_in);
        if signed_in {
            assert_eq!(value["identity"]["uid"], "local-dev-user");
        }
        assert_eq!(
            codeterm_account::protected_credentials::ProtectedCredentials::new(fixture.temp.path())
                .load("domios-account-local-refresh")
                .unwrap()
                .is_some(),
            signed_in
        );
        assert!(!store.has("domios-account-local-refresh").await.unwrap());
        assert!(value.get("id_token").is_none());
    }
}

#[test]
fn account_plan_routes_payload_and_local_admission() {
    assert!(serde_json::from_value::<Upgrade>(serde_json::json!({"plan":"pro"})).is_ok());
    assert!(
        serde_json::from_value::<Upgrade>(serde_json::json!({"plan":"pro","uid":"foreign"}))
            .is_err()
    );
    assert_eq!(admit(None).unwrap_err().0, StatusCode::FORBIDDEN);
    let snapshot = codeterm_billing::Snapshot::default();
    let value = serde_json::to_value(snapshot).unwrap();
    assert_eq!(value["entitlements"]["plan"], "free");
    assert_eq!(value["stale"], false);
}

#[tokio::test]
async fn account_plan_routes_local_journey() {
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use std::sync::Arc;
    use tower::ServiceExt;
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(
        codeterm_service_core::multiplexer::state::MultiplexerState::empty(),
    );
    *fixture.service.secrets.write().await =
        Arc::new(codeterm_service_core::secrets::mock::MockStore::new());
    let provider = Arc::new(codeterm_billing::adapters::fake::Fake::default());
    *provider.value.lock() = Ok(codeterm_billing::Entitlements {
        plan: codeterm_billing::Plan::Pro,
        current_period_end: 2000000000,
        ..Default::default()
    });
    assert!(fixture
        .service
        .ext::<codeterm_service_core::service::billing::BillingCell>()
        .0
        .set(Arc::new(codeterm_billing::Billing::new(provider.clone())))
        .is_ok());
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    state.config.lock().api.token = Some("fixture-token".into());
    state.config.lock().account = serde_json::json!({"provider":"local"});
    let router = crate::routes::create_router(state);
    for (method, path, expected_plan, stale) in [
        ("GET", "/api/account/plan", "free", false),
        ("POST", "/api/account/login", "", false),
        ("GET", "/api/account/plan", "free", false),
        ("POST", "/api/account/logout", "", false),
        ("GET", "/api/account/plan", "free", false),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("authorization", "Bearer fixture-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        assert_eq!(
            status,
            StatusCode::OK,
            "{method} {path}: {}",
            String::from_utf8_lossy(&body)
        );
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        if !expected_plan.is_empty() {
            assert_eq!(value["entitlements"]["plan"], expected_plan);
            assert_eq!(value["stale"], stale);
            assert_eq!(value["limits"]["enforcing"], true);
            assert_eq!(value["limits"]["agent.tabs"], 35);
        }
    }
    assert!(!fixture
        .service
        .secrets
        .read()
        .await
        .has("domios-account-local-refresh")
        .await
        .unwrap());
}

fn callback_request(state: &str) -> Json<AccountCallback> {
    Json(AccountCallback {
        url: format!(
            "domios://auth/callback?code={}&state={state}",
            "c".repeat(43)
        ),
    })
}

#[tokio::test]
async fn account_callback_refuses_nonlocal_before_completion() {
    let calls = std::cell::Cell::new(0);
    for caller in [
        None,
        Some(Extension(CallerIdentity::ServicePush {
            service_id: "fixture".into(),
        })),
        Some(Extension(CallerIdentity::Peer {
            instance_id: "fixture".into(),
            access_level: codeterm_service_core::mesh::types::AccessLevel::Full,
            relay_protocol_version: 0,
        })),
    ] {
        let error = dispatch_callback(
            caller.as_ref(),
            Ok(callback_request("fixture-pending-state")),
            |_| {
                calls.set(calls.get() + 1);
                std::future::ready(Ok(()))
            },
        )
        .await
        .unwrap_err();
        assert_eq!(error.0, StatusCode::FORBIDDEN);
    }
    assert_eq!(calls.get(), 0);
}

#[tokio::test]
async fn account_callback_refuses_invalid_or_oversized_url_before_completion() {
    let local = Extension(CallerIdentity::Local);
    let calls = std::cell::Cell::new(0);
    for (url, status) in [
        ("secret-malformed-callback".into(), StatusCode::BAD_REQUEST),
        (
            "https://foreign.invalid/callback?secret=sentinel".into(),
            StatusCode::BAD_REQUEST,
        ),
        (
            "x".repeat(MAX_CALLBACK_URL_BYTES + 1),
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
    ] {
        let error = dispatch_callback(Some(&local), Ok(Json(AccountCallback { url })), |_| {
            calls.set(calls.get() + 1);
            std::future::ready(Ok(()))
        })
        .await
        .unwrap_err();
        assert_eq!(error.0, status);
        assert!(!error.1.contains("secret"));
    }
    assert_eq!(calls.get(), 0);
}

#[tokio::test]
async fn account_callback_forwards_once_and_only_acknowledges() {
    let local = Extension(CallerIdentity::Local);
    let calls = std::cell::Cell::new(0);
    let status = dispatch_callback(
        Some(&local),
        Ok(callback_request("fixture-pending-state")),
        |callback| {
            calls.set(calls.get() + 1);
            assert_eq!(callback.code, "c".repeat(43));
            assert_eq!(callback.state, "fixture-pending-state");
            std::future::ready(Ok(()))
        },
    )
    .await
    .unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn account_callback_preserves_pending_state_pkce_and_single_consumption() {
    let local = Extension(CallerIdentity::Local);
    let mut flow = codeterm_account::LoginFlow::from_parts(
        "fixture-current-state".into(),
        codeterm_account::Secret::new("fixture-verifier"),
        "domios://auth/callback".into(),
    );
    let accepted = std::cell::Cell::new(0);
    for (state, expected) in [
        ("fixture-wrong-state", StatusCode::CONFLICT),
        ("fixture-stale-state", StatusCode::CONFLICT),
        ("fixture-current-state", StatusCode::NO_CONTENT),
        ("fixture-current-state", StatusCode::CONFLICT),
    ] {
        let result = dispatch_callback(Some(&local), Ok(callback_request(state)), |callback| {
            std::future::ready(
                flow.complete(callback)
                    .map(|redemption| {
                        accepted.set(accepted.get() + 1);
                        assert_eq!(redemption.verifier.expose(), "fixture-verifier");
                        assert_eq!(redemption.redirect, "domios://auth/callback");
                    })
                    .map_err(callback_failure),
            )
        })
        .await;
        assert_eq!(result.unwrap_or_else(|error| error.0), expected);
    }
    assert_eq!(accepted.get(), 1);
}

#[tokio::test]
async fn account_callback_http_rejects_before_initializing_account() {
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use tower::ServiceExt;
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(
        codeterm_service_core::multiplexer::state::MultiplexerState::empty(),
    );
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    state.config.lock().api.token = Some("callback-fixture-token".into());
    let router = crate::routes::create_router(state);
    let valid_body = serde_json::json!({"url": format!("domios://auth/callback?code={}&state=fixture-pending-state", "c".repeat(43))}).to_string();
    for (method, token, body, expected) in [
        ("POST", None, valid_body.clone(), StatusCode::UNAUTHORIZED),
        (
            "POST",
            Some("wrong-token"),
            valid_body.clone(),
            StatusCode::UNAUTHORIZED,
        ),
        (
            "GET",
            Some("callback-fixture-token"),
            valid_body,
            StatusCode::METHOD_NOT_ALLOWED,
        ),
        (
            "POST",
            Some("callback-fixture-token"),
            "{secret-malformed-json".into(),
            StatusCode::BAD_REQUEST,
        ),
        (
            "POST",
            Some("callback-fixture-token"),
            r#"{"url":"secret-malformed-url","extra":"secret-extra"}"#.into(),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "POST",
            Some("callback-fixture-token"),
            r#"{"url":"secret-malformed-url"}"#.into(),
            StatusCode::BAD_REQUEST,
        ),
        (
            "POST",
            Some("callback-fixture-token"),
            serde_json::json!({"url": "x".repeat(MAX_CALLBACK_REQUEST_BYTES)}).to_string(),
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
    ] {
        let mut request = Request::builder()
            .method(method)
            .uri("/api/account/callback")
            .header("content-type", "application/json");
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let response = router
            .clone()
            .oneshot(request.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("secret"));
        assert!(fixture
            .service
            .ext::<codeterm_service_core::service::account::AccountCell>()
            .0
            .get()
            .is_none());
    }
}

#[test]
fn account_upgrade_opens_the_plain_checkout_for_an_unknown_theme() {
    for body in [
        r#"{"plan":"pro_monthly","theme":"system"}"#,
        r#"{"plan":"pro_monthly","theme":"Dark"}"#,
        r#"{"plan":"pro_monthly"}"#,
    ] {
        let upgrade: Upgrade = serde_json::from_str(body).unwrap();
        assert_eq!(upgrade.checkout_theme(), None, "{body}");
    }
    let upgrade: Upgrade =
        serde_json::from_str(r#"{"plan":"pro_monthly","theme":"light"}"#).unwrap();
    assert_eq!(
        upgrade.checkout_theme(),
        Some(codeterm_billing::CheckoutTheme::Light)
    );
}

#[tokio::test]
async fn explicit_plan_refresh_fetches_each_time_while_passive_focus_is_throttled() {
    use axum::{body::Body, http::Request};
    use std::sync::Arc;
    use tower::ServiceExt;
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(
        codeterm_service_core::multiplexer::state::MultiplexerState::empty(),
    );
    *fixture.service.secrets.write().await =
        Arc::new(codeterm_service_core::secrets::mock::MockStore::new());
    let provider = Arc::new(codeterm_billing::adapters::fake::Fake::default());
    assert!(fixture
        .service
        .ext::<codeterm_service_core::service::billing::BillingCell>()
        .0
        .set(Arc::new(codeterm_billing::Billing::new(provider.clone())))
        .is_ok());
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    state.config.lock().api.token = Some("fixture-token".into());
    state.config.lock().account = serde_json::json!({"provider":"local"});
    let router = crate::routes::create_router(state);
    for path in [
        "/api/account/login",
        "/api/account/plan/refresh",
        "/api/account/plan/refresh",
    ] {
        let before = *provider.calls.lock();
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header("authorization", "Bearer fixture-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(*provider.calls.lock(), before + 1);
    }
    let before = *provider.calls.lock();
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/account/plan/refresh?focus=true")
                .header("authorization", "Bearer fixture-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(*provider.calls.lock(), before);
}

#[tokio::test]
async fn portal_no_billing_customer_keeps_its_code_and_message() {
    let response = portal_failure(codeterm_billing::NO_BILLING_CUSTOMER.into());
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
        serde_json::json!({
            "code": "no_billing_customer",
            "message": "There is no paid subscription to manage. Your current access is not billed through Paddle."
        })
    );
    assert_eq!(
        portal_failure("unavailable".into()).status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn receiver_refusals_retain_machine_readable_codes() {
    for code in [
        "receiver_elsewhere",
        "receiver_revision_stale",
        "not_receiver",
        "recipient_not_receiving",
        "installation_unverified",
        "operation_id_conflict",
        "session_revoked",
    ] {
        let message = format!("{code}: opaque explanation");
        let response = neighbor_failure(message.clone());
        assert_eq!(
            response.status(),
            if code == "session_revoked" {
                StatusCode::UNAUTHORIZED
            } else {
                StatusCode::CONFLICT
            }
        );
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            body,
            serde_json::json!({"code":code,"reason":code,"message":message,"error":message})
        );
    }
    assert_eq!(
        neighbor_failure("unavailable".into()).status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn refusal_preflight_errors_report_nothing_sent_and_keep_byte_counts() {
    for (error, status, reason) in [
        (
            "refusal_too_long: {\"limit\":1024,\"actual\":1025}",
            StatusCode::PAYLOAD_TOO_LARGE,
            "refusal_too_long",
        ),
        (
            "reply_target_unknown: Nothing was sent. Reply to a known incoming message",
            StatusCode::BAD_REQUEST,
            "reply_target_unknown",
        ),
    ] {
        let response = neighbor_failure(error.to_string());
        assert_eq!(response.status(), status);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["code"], reason);
        assert!(body["message"]
            .as_str()
            .unwrap()
            .contains("Nothing was sent"));
        if reason == "refusal_too_long" {
            assert_eq!(body["limit"], 1024);
            assert_eq!(body["actual"], 1025);
        }
    }
}

#[tokio::test]
async fn neighbor_flags_have_a_typed_local_api_contract_without_body_fields() {
    use codeterm_service_core::service::neighbors::{FlagCategory, NeighborRuntimeCommand};
    let request = serde_json::json!({"operation":"flag_message","friendship_id":"a".repeat(64),"message_id":"1".repeat(32),"category":"spam","reason":"Unsolicited repetition"});
    let command: NeighborRuntimeCommand = serde_json::from_value(request.clone()).unwrap();
    assert!(matches!(
        command,
        NeighborRuntimeCommand::FlagMessage {
            category: FlagCategory::Spam,
            ..
        }
    ));
    let mut invalid = request.clone();
    invalid["body"] = serde_json::json!("private content");
    assert!(serde_json::from_value::<NeighborRuntimeCommand>(invalid).is_err());
    let mut invalid = request;
    invalid["category"] = serde_json::json!("unknown");
    assert!(serde_json::from_value::<NeighborRuntimeCommand>(invalid).is_err());
    for (reason, status) in [
        ("invalid_flag", StatusCode::BAD_REQUEST),
        ("neighbor_flag_limit", StatusCode::CONFLICT),
        ("neighbor_forbidden", StatusCode::FORBIDDEN),
    ] {
        let response = neighbor_failure(format!("{reason}: Local flag refused"));
        assert_eq!(response.status(), status);
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["reason"],
            reason
        );
    }
}

#[tokio::test]
async fn stopped_reply_is_a_typed_presend_conflict() {
    let response = neighbor_failure(
        "reply_target_stopped: The owner stopped this request; nothing was sent".into(),
    );
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["code"], "reply_target_stopped");
    assert_eq!(body["reason"], "reply_target_stopped");
    assert!(body["message"]
        .as_str()
        .unwrap()
        .contains("nothing was sent"));
}

#[tokio::test]
async fn empty_neighbor_body_is_a_typed_presend_bad_request() {
    let response = neighbor_failure(
        "empty_body: The message is empty; nothing was sent. Provide text or an attachment.".into(),
    );
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["code"], "empty_body");
    assert_eq!(body["reason"], "empty_body");
    assert!(body["message"]
        .as_str()
        .unwrap()
        .contains("nothing was sent"));
}

#[tokio::test]
async fn invalid_neighbor_permission_has_typed_bad_request_response() {
    let response = neighbor_failure("invalid_request: Invalid Neighbor request".into());
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["code"], "invalid_request");
    assert_eq!(body["reason"], "invalid_request");
}

#[tokio::test]
async fn nonreceiver_concierge_mutation_is_a_typed_conflict() {
    let response = neighbor_failure(
        "receiving_elsewhere: This machine is not receiving; no concierge can run here".into(),
    );
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["code"], "receiving_elsewhere");
    assert_eq!(body["reason"], "receiving_elsewhere");
}

#[tokio::test]
async fn friend_receiver_machine_limit_preserves_typed_counts() {
    let response = neighbor_failure("key_limit: {\"limit\":5,\"active\":5}".into());
    assert_eq!(response.status(),StatusCode::CONFLICT);
    let bytes = axum::body::to_bytes(response.into_body(),1024*1024).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["code"],"key_limit");
    assert_eq!(body["limit"],5);
    assert_eq!(body["active"],5);
}
