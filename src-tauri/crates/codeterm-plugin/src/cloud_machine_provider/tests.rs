use super::*;

#[test]
fn cloud_machine_provider_preview_validates_origin_and_redacts_the_header() {
    let token = "fixture-sensitive-header";
    let access = CloudPreviewAccess::new(
        "https://example.test",
        "x-preview-token",
        token,
        AccessLifetime::RotatesOnRestart,
    )
    .unwrap();
    assert_eq!(access.origin().as_str(), "https://example.test/");
    assert_eq!(access.header().1, token);
    assert!(access.header().1.is_sensitive());
    assert!(!format!("{access:?}").contains(token));
    assert_eq!(access.lifetime(), AccessLifetime::RotatesOnRestart);
    for origin in [
        "http://example.test",
        "https://user:password@example.test",
        "https://example.test/route",
        "https://example.test/?token=value",
        "https://example.test/#fragment",
    ] {
        assert!(
            CloudPreviewAccess::new(origin, "x-token", token, AccessLifetime::Unspecified).is_err()
        );
    }
}

#[test]
fn cloud_machine_provider_direct_refuses_internal_addresses() {
    for ip in [
        "127.0.0.1",
        "10.1.2.3",
        "169.254.169.254",
        "100.100.100.100",
        "224.0.0.1",
    ] {
        assert!(CloudAccess::direct(ip, 7687).is_err());
    }
    let direct = CloudAccess::direct("1.1.1.1", 7687).unwrap();
    assert_eq!(direct.lifetime(), AccessLifetime::Stable);
}
