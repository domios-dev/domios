use super::{write_cli_error, CliError};

#[test]
fn account_mesh_refusal_uses_a_repair_hint_without_echoing_http_json() {
    for (reason, expected, hint) in [
        (
            "unauthenticated",
            "account session could not be verified",
            "sign in again",
        ),
        (
            "verification-unavailable",
            "membership could not be verified",
            "sign in again",
        ),
        (
            "future-refusal",
            "Account Mesh refused the link (future-refusal).",
            "refresh the account machine list",
        ),
    ] {
        let body =
            format!(r#"{{"ok":false,"request_id":"request-1","failure":{{"reason":"{reason}"}}}}"#);
        let error = CliError::http(401, &body);
        let mut out = Vec::new();
        let mut err = Vec::new();
        write_cli_error(false, &error, &mut out, &mut err);
        let rendered = String::from_utf8(err).unwrap();

        assert!(rendered.contains(expected), "{rendered}");
        assert!(rendered.contains(hint), "{rendered}");
        assert!(!rendered.contains("request-1"), "{rendered}");
        assert!(!rendered.contains("HTTP 401"), "{rendered}");
        assert!(!rendered.contains("failure"), "{rendered}");
    }
}
