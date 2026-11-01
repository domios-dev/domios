use super::*;

#[test]
fn focus_action_accepts_only_the_bare_production_link() {
    assert!(is_focus_action(&url::Url::parse("domios://focus").unwrap()));
    for value in [
        "domios://focus?code=opaque",
        "domios://focus#fragment",
        "domios://focus/other",
        "domios://user@focus",
        "domios-dev://focus",
        "https://focus",
    ] {
        assert!(!is_focus_action(&url::Url::parse(value).unwrap()));
    }
}

#[test]
fn registration_repair_requires_production_and_a_mismatched_handler() {
    assert!(registration_needs_repair(true, false));
    assert!(!registration_needs_repair(true, true));
    assert!(!registration_needs_repair(false, false));
    assert!(!registration_needs_repair(false, true));
}

#[test]
fn only_default_production_instance_can_register_or_handle_production_links() {
    assert!(production_instance(
        false,
        None,
        None,
        None,
        "com.domios.app"
    ));
    for (debug, dev, instance, port, identifier) in [
        (true, None, None, None, "com.domios.app"),
        (false, Some("1"), None, None, "com.domios.app"),
        (false, None, Some("alternate"), None, "com.domios.app"),
        (false, None, None, Some("7686"), "com.domios.app"),
        (false, None, None, None, "com.domios.dev"),
    ] {
        assert!(!production_instance(debug, dev, instance, port, identifier));
    }
}

#[test]
fn account_callback_candidates_are_forwarded_without_client_validation() {
    let url = url::Url::parse("domios://auth/callback?code=invalid&state=short").unwrap();

    assert!(is_account_callback(&url));
    assert_eq!(billing_return_status(&url), None);
}

#[test]
fn billing_return_requires_the_original_confined_shape() {
    let accepted = url::Url::parse("domios://billing/return?status=success").unwrap();
    let rejected = url::Url::parse("domios://billing/return?status=success&status=again").unwrap();

    assert_eq!(
        billing_return_status(&accepted),
        Some(Some("success".into()))
    );
    assert_eq!(billing_return_status(&rejected), None);
}
