use super::*;

#[test]
fn recovery_admission_requires_current_ownership_before_tolerating_unreadable_urls() {
    let unavailable = unavailable_url().unwrap();
    let unreadable = || Err("invalid url: relative URL without a base".into());
    assert!(recovery_document(unreadable(), &unavailable, true, (2, 3), (2, 3), true).unwrap());
    assert!(recovery_document(unreadable(), &unavailable, false, (2, 3), (2, 3), true).is_err());
    for (current, attached) in [
        ((1, 3), true),
        ((2, 4), true),
        ((2, 3), false),
        ((0, 3), true),
    ] {
        assert!(
            recovery_document(unreadable(), &unavailable, true, current, (2, 3), attached).is_err()
        );
    }
    assert!(!recovery_document(
        Ok(Url::parse("http://127.0.0.1:7716/bundled/index.html").unwrap()),
        &unavailable,
        false,
        (2, 3),
        (2, 3),
        true,
    )
    .unwrap());
    assert!(recovery_document(
        Ok(unavailable.clone()),
        &unavailable,
        false,
        (2, 3),
        (2, 3),
        true
    )
    .unwrap());
}
