use super::*;

#[test]
fn fallback_actions_match_supported_states() {
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
    assert!(page.contains(r#"role="alert""#));
    assert!(page.contains("location.hash"));
    assert!(page.contains("domios-shell://retry"));
    assert!(page.contains("domios-shell://quit"));
    assert!(!page.contains("setInterval"));
}

#[test]
fn starting_surface_stays_passive_until_readiness() {
    let page = brand_fallback_page(BrandFallback::Starting);
    assert!(page.contains(r#"role="status""#));
    assert!(!page.contains("domios-shell://retry"));
    assert!(!page.contains("setInterval"));
    assert!(!page.contains("setTimeout"));
}
