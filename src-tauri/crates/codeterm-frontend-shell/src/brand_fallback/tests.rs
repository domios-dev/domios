use super::*;

#[test]
fn both_pages_share_the_mark_and_fill_every_slot() {
    for kind in [BrandFallback::Reconnecting, BrandFallback::InterfaceMissing] {
        let page = brand_fallback_page(kind);
        assert!(page.contains("M8 8H32C46 8 56 18 56 32S46 56 32 56H24L40 40V24H24V40L8 56Z"));
        assert!(!page.contains("{{"));
    }
    let reconnecting = brand_fallback_page(BrandFallback::Reconnecting);
    assert!(reconnecting.contains("domios-shell://retry"));
    assert!(reconnecting.contains("domios-shell://close"));
    let missing = brand_fallback_page(BrandFallback::InterfaceMissing);
    assert!(!missing.contains("<script"));
    assert!(!missing.contains("domios-shell://"));
}

#[test]
fn start_failure_offers_a_manual_retry_and_a_reason_slot() {
    let page = brand_fallback_page(BrandFallback::StartFailed);
    assert!(!page.contains("{{"));
    assert!(page.contains(r#"role="alert""#));
    assert!(page.contains(r#"class="detail""#));
    assert!(page.contains("location.hash"));
    assert!(page.contains("domios-shell://retry"));
    assert!(page.contains("domios-shell://quit"));
    assert!(!page.contains("setInterval"));
}

#[test]
fn starting_surface_is_informative_without_readiness_timers_or_retry() {
    let page = brand_fallback_page(BrandFallback::Starting);
    assert!(page.contains("Starting Domios"));
    assert!(page.contains("Restoring your sessions."));
    assert!(page.contains(r#"role="status""#));
    assert!(!page.contains("domios-shell://retry"));
    assert!(!page.contains("setInterval"));
    assert!(!page.contains("setTimeout"));
}
