use super::BubbleCache;

#[test]
fn unchanged_completions_do_not_invalidate_consumers() {
    let mut cache = BubbleCache::<Option<String>>::default();
    for (value, changed) in [
        (Some(None), true),
        (Some(None), false),
        (Some(Some("ready".into())), true),
        (Some(Some("ready".into())), false),
        (None, false),
        (Some(None), true),
        (Some(None), false),
    ] {
        assert!(cache.request("tab", true).1);
        let outcome = cache.finish("tab", value);
        assert_eq!(outcome.changed, changed);
        assert!(!outcome.refresh_again);
    }
}

#[test]
fn cache_coalesces_and_returns_last_value_during_refresh() {
    let mut cache = BubbleCache::<Option<String>>::default();
    assert_eq!(cache.request("repo", false), (None, true));
    for _ in 0..221 {
        assert_eq!(cache.request("repo", false), (None, false));
    }
    assert!(
        !cache
            .finish("repo", Some(Some("main".into())))
            .refresh_again
    );
    assert_eq!(
        cache.request("repo", false),
        (Some(Some("main".into())), false)
    );
    assert_eq!(
        cache.request("repo", true),
        (Some(Some("main".into())), true)
    );
    assert_eq!(
        cache.request("repo", false),
        (Some(Some("main".into())), false)
    );
    assert!(
        !cache
            .finish("repo", Some(Some("next".into())))
            .refresh_again
    );
    assert_eq!(
        cache.request("repo", false),
        (Some(Some("next".into())), false)
    );
}

#[test]
fn invalidations_during_refresh_schedule_one_trailing_refresh() {
    let mut cache = BubbleCache::<String>::default();
    assert!(cache.request("a", false).1);
    assert!(cache.request("b", false).1);
    for _ in 0..50 {
        assert!(!cache.request("a", true).1);
    }
    assert!(cache.finish("a", Some("old".into())).refresh_again);
    assert!(!cache.request("a", false).1);
    assert!(!cache.finish("a", Some("fresh".into())).refresh_again);
    assert_eq!(cache.request("a", false), (Some("fresh".into()), false));
    assert!(!cache.finish("b", Some("independent".into())).refresh_again);
}

#[test]
fn null_is_cached_and_failures_preserve_last_value() {
    let mut cache = BubbleCache::<Option<String>>::default();
    cache.request("repo", false);
    cache.finish("repo", Some(None));
    assert_eq!(cache.request("repo", false), (Some(None), false));
    cache.request("repo", true);
    cache.finish("repo", None);
    assert_eq!(cache.request("repo", false), (Some(None), false));
}
