use super::*;

#[test]
fn legacy_search_requires_success_before_reporting_absence() {
    assert!(matches!(
        legacy_search_result(false, ""),
        Err(Error::Credentials)
    ));
    assert!(legacy_search_result(true, "").unwrap().is_none());
    assert_eq!(
        legacy_search_result(true, "label = opaque\nsecret = found\n")
            .unwrap()
            .unwrap()
            .expose(),
        "found"
    );
}
