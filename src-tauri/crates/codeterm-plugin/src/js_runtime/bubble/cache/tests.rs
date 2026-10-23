use super::BubbleCache;

#[test]
fn cache_coalesces_and_returns_last_value_during_refresh() {
    let mut cache = BubbleCache::<Option<String>>::default();
    assert_eq!(cache.request("repo", false), (None, true));
    for _ in 0..221 {
        assert_eq!(cache.request("repo", false), (None, false));
    }
    assert!(!cache.finish("repo", Some(Some("main".into()))));
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
    assert!(!cache.finish("repo", Some(Some("next".into()))));
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
    assert!(cache.finish("a", Some("old".into())));
    assert!(!cache.request("a", false).1);
    assert!(!cache.finish("a", Some("fresh".into())));
    assert_eq!(cache.request("a", false), (Some("fresh".into()), false));
    assert!(!cache.finish("b", Some("independent".into())));
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
