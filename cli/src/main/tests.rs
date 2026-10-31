use super::restricted_cli_args;
use std::ffi::{OsStr, OsString};

#[test]
fn restricted_cli_refuses_global_overrides_and_extra_arguments() {
    for args in [
        vec!["cli", "identify", "--json"],
        vec!["cli", "identify", "--json", "--mesh=foreign"],
        vec!["cli", "identify", "--json", "--mesh", "foreign"],
        vec!["cli", "--json", "identify"],
        vec!["cli", "identify", "--json", "--verbose"],
        vec!["cli", "identify"],
        vec!["cli", "account", "neighbors", "send", "foreign"],
    ] {
        let expected = args == ["cli", "identify", "--json"];
        let args = args.into_iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(
            restricted_cli_args(Some(OsStr::new("identify-only")), &args).is_ok(),
            expected
        );
        assert!(restricted_cli_args(Some(OsStr::new("unknown")), &args).is_err());
        assert!(restricted_cli_args(None, &args).is_ok());
    }
}
