use super::*;

#[test]
fn billing_return_selects_only_refresh_and_opaque_status() {
    for (input, expected) in [
        ("domios://billing/return", None),
        (
            "domios://billing/return?status=completed",
            Some("completed"),
        ),
        ("domios://billing/return?status=cancel_1", Some("cancel_1")),
    ] {
        let url = url::Url::parse(input).unwrap();
        match deep_link_action(&url).unwrap() {
            DeepLinkAction::BillingReturn { status } => assert_eq!(status.as_deref(), expected),
            DeepLinkAction::AccountCallback(_) => panic!("Billing return selected authentication"),
        }
    }
}

#[test]
fn billing_return_refuses_identity_entitlement_and_redirect_parameters() {
    for input in [
        "domios://billing/return?uid=owner",
        "domios://billing/return?plan=pro",
        "domios://billing/return?status=active&status=canceled",
        "domios://billing/return?status=",
        "domios://billing/return?status=https%3A%2F%2Fexample.com",
        "domios://billing/return?status=active&redirect=https://example.com",
        "domios://billing/return#status=active",
        "domios://owner@billing/return",
        "domios://billing:123/return",
        "domios://billing/return/",
        "https://billing/return",
        "domios://foreign/return",
    ] {
        assert!(
            deep_link_action(&url::Url::parse(input).unwrap()).is_none(),
            "{input}"
        );
    }
    let long = format!("domios://billing/return?status={}", "x".repeat(65));
    assert!(deep_link_action(&url::Url::parse(&long).unwrap()).is_none());
}

#[test]
fn auth_callback_keeps_its_existing_dispatch() {
    let input = format!(
        "domios://auth/callback?code={}&state=opaque-state-matching",
        "a".repeat(43)
    );
    let url = url::Url::parse(&input).unwrap();
    assert!(matches!(
        deep_link_action(&url),
        Some(DeepLinkAction::AccountCallback(_))
    ));
}
