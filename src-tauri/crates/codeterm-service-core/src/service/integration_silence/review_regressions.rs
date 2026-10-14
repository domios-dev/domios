use super::*;

#[test]
fn fallback_retry_is_released_after_failure_and_consumed_after_success() {
    let mut fallbacks = HashMap::new();
    let mut sessions = HashMap::new();
    sessions.insert(
        "session".to_string(),
        HookBindingFallback {
            source: BindingSource::LaunchMarker,
            session_start_retry_used: false,
            session_start_retry_reserved: false,
            sequence: 1,
        },
    );
    fallbacks.insert("fixture-provider".to_string(), sessions);

    assert!(reserve_hook_binding_fallback_retry_from(
        &mut fallbacks,
        "fixture-provider",
        "session",
    ));
    settle_hook_binding_fallback_retry(&mut fallbacks, "fixture-provider", "session", false);
    assert!(reserve_hook_binding_fallback_retry_from(
        &mut fallbacks,
        "fixture-provider",
        "session",
    ));
    settle_hook_binding_fallback_retry(&mut fallbacks, "fixture-provider", "session", true);
    assert!(!reserve_hook_binding_fallback_retry_from(
        &mut fallbacks,
        "fixture-provider",
        "session",
    ));
}

#[test]
fn dropped_fallback_retry_can_be_reserved_again() {
    let provider_id = "review-regression-provider";
    let session_id = "review-regression-session";
    clear_hook_binding_fallback_session(provider_id, session_id);
    {
        let mut fallbacks = hook_binding_fallbacks().lock();
        update_hook_binding_fallback(
            &mut fallbacks,
            provider_id,
            session_id,
            BindingSource::LaunchMarker,
            false,
        );
    }

    let retry = reserve_hook_binding_fallback_retry(provider_id, session_id).unwrap();
    drop(retry);
    let retried = reserve_hook_binding_fallback_retry(provider_id, session_id).unwrap();
    retried.commit();
    assert!(reserve_hook_binding_fallback_retry(provider_id, session_id).is_none());
    clear_hook_binding_fallback_session(provider_id, session_id);
}

#[test]
fn fallback_registry_is_bounded_and_clears_sessions_across_providers() {
    let mut fallbacks = HookBindingFallbacks::new();
    for index in 0..=MAX_HOOK_BINDING_FALLBACKS {
        update_hook_binding_fallback(
            &mut fallbacks,
            "fixture-provider",
            &format!("session-{index}"),
            BindingSource::LaunchMarker,
            false,
        );
    }
    assert_eq!(
        fallbacks.values().map(HashMap::len).sum::<usize>(),
        MAX_HOOK_BINDING_FALLBACKS,
    );
    assert!(!fallbacks["fixture-provider"].contains_key("session-0"));
    assert!(fallbacks["fixture-provider"].contains_key("session-1"));

    update_hook_binding_fallback(
        &mut fallbacks,
        "other-provider",
        "session-1",
        BindingSource::PidRegistry,
        false,
    );
    clear_hook_binding_fallback_from(&mut fallbacks, "fixture-provider", "session-1");
    assert!(!fallbacks["fixture-provider"].contains_key("session-1"));
    assert!(fallbacks["other-provider"].contains_key("session-1"));
    clear_hook_binding_fallback_from(&mut fallbacks, "other-provider", "session-1");
    assert!(!fallbacks.contains_key("other-provider"));
}
