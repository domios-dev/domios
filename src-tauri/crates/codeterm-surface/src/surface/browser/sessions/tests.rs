use super::*;
#[test]
fn named_sessions_open_the_instance_root() {
    let args =
        parse(&["--profile", "dev-qa", "--origin", "http://127.0.0.1:7790/"].map(String::from))
            .unwrap();
    assert_eq!(args.ui_url(), "http://127.0.0.1:7790/");
}

#[test]
fn named_session_arguments_require_an_explicit_origin_and_profile() {
    let args = [
        "--profile",
        "dev-qa",
        "--origin",
        "http://localhost:7686",
        "--expect-build",
        "opaque-sha",
        "--ephemeral",
    ]
    .map(String::from);
    assert_eq!(
        parse(&args).unwrap(),
        SessionArgs {
            profile: "dev-qa".into(),
            origin: "http://localhost:7686".into(),
            build: Some("opaque-sha".into()),
            ephemeral: true
        }
    );
    assert!(parse(&["--profile".into(), "dev".into()]).is_err());
    assert!(parse(&["--origin".into(), "http://localhost:7686".into()]).is_err());
    assert!(parse(&["--token".into(), "secret".into()]).is_err());
}
