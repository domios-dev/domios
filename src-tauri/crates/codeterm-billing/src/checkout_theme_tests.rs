use crate::{checkout_url_with_theme, CheckoutTheme};

const FRAGMENT: &str = "state=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA&transaction_id=txn_01hv8y3k2m4n5p6q7r8s9t0vwx";

fn checkout(query: &str) -> url::Url {
    url::Url::parse(&format!("https://domios.dev/checkout?{query}#{FRAGMENT}")).unwrap()
}

#[test]
fn appends_the_theme_and_keeps_ptxn_and_fragment() {
    let url = checkout_url_with_theme(
        checkout("_ptxn=txn_01hv8y3k2m4n5p6q7r8s9t0vwx"),
        CheckoutTheme::Dark,
    );
    assert_eq!(
        url.as_str(),
        format!("https://domios.dev/checkout?_ptxn=txn_01hv8y3k2m4n5p6q7r8s9t0vwx&theme=dark#{FRAGMENT}")
    );
}

#[test]
fn replaces_an_existing_theme_and_keeps_other_parameters_in_order() {
    let url = checkout_url_with_theme(
        checkout("theme=dark&_ptxn=txn_01hv8y3k2m4n5p6q7r8s9t0vwx&theme=system&x=1"),
        CheckoutTheme::Light,
    );
    assert_eq!(
        url.query(),
        Some("_ptxn=txn_01hv8y3k2m4n5p6q7r8s9t0vwx&x=1&theme=light")
    );
    assert_eq!(url.fragment(), Some(FRAGMENT));
}

#[test]
fn accepts_only_resolved_light_or_dark() {
    assert_eq!(
        serde_json::from_str::<CheckoutTheme>("\"light\"").unwrap(),
        CheckoutTheme::Light
    );
    assert_eq!(
        serde_json::from_str::<CheckoutTheme>("\"dark\"").unwrap(),
        CheckoutTheme::Dark
    );
    for value in ["\"system\"", "\"Dark\"", "\"\"", "1"] {
        assert!(
            serde_json::from_str::<CheckoutTheme>(value).is_err(),
            "{value}"
        );
    }
}

#[test]
fn appending_keeps_the_existing_query_bytes() {
    let url = checkout_url_with_theme(checkout("sig=a%20b&flag&_ptxn=txn_1"), CheckoutTheme::Dark);
    assert_eq!(url.query(), Some("sig=a%20b&flag&_ptxn=txn_1&theme=dark"));
}

#[test]
fn parse_maps_unknown_themes_to_none() {
    assert_eq!(CheckoutTheme::parse("light"), Some(CheckoutTheme::Light));
    assert_eq!(CheckoutTheme::parse("dark"), Some(CheckoutTheme::Dark));
    for value in ["system", "Dark", ""] {
        assert_eq!(CheckoutTheme::parse(value), None, "{value}");
    }
}

#[test]
fn replacing_keeps_the_other_query_bytes() {
    let url = checkout_url_with_theme(checkout("sig=a%20b&theme=light&flag"), CheckoutTheme::Dark);
    assert_eq!(url.query(), Some("sig=a%20b&flag&theme=dark"));
}
