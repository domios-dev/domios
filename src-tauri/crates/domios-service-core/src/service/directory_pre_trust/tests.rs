use super::authorized_cwd;

#[test]
fn directory_pre_trust_requires_authority_and_the_exact_launch_cwd() {
    for (authorized, tab, launch, expected) in [
        (true, Some("/exact"), Some("/exact"), Some("/exact")),
        (true, Some("/exact"), None, Some("/exact")),
        (false, Some("/exact"), Some("/exact"), None),
        (true, Some("/exact"), Some("/exact/child"), None),
        (true, None, Some("/exact"), None),
        (true, Some(""), None, None),
    ] {
        assert_eq!(authorized_cwd(authorized, tab, launch), expected);
    }
}
