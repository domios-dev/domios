use super::*;

#[test]
fn runtime_identity_requires_debug_development_and_validated_values() {
    for (debug, development) in [(false, true), (true, false), (false, false)] {
        assert_eq!(
            development_runtime_frontend(debug, development, "invalid", "foreign").unwrap(),
            None
        );
    }
    assert_eq!(
        development_runtime_frontend(true, true, "", "").unwrap(),
        None
    );
    for (port, identifier) in [
        ("1821", "com.domios.dev.first"),
        ("1822", "com.domios.dev.second"),
    ] {
        let actual = development_runtime_frontend(true, true, port, identifier)
            .unwrap()
            .unwrap();
        assert_eq!(actual.origin.as_str(), format!("http://127.0.0.1:{port}/"));
        assert_eq!(actual.identifier, identifier);
    }
    for port in ["0", "65536", "-1", "1821/path", " 1821"] {
        assert!(development_runtime_frontend(true, true, port, "com.domios.dev.first").is_err());
    }
    for identifier in ["", "com.domios.app", "foreign", "com.domios.dev.bad/path"] {
        assert!(development_runtime_frontend(true, true, "1821", identifier).is_err());
    }
}

#[test]
fn browser_runtime_arguments_preserve_explicit_options_and_add_instance_defaults() {
    let defaults = development_browser_arguments("9321", "").unwrap();
    assert!(defaults.contains("--remote-debugging-port=9321"));
    assert!(defaults.contains("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection"));
    let custom = development_browser_arguments(
        "9322",
        "--opaque=one --disable-features=opaque --remote-debugging-port=9323",
    )
    .unwrap();
    assert_eq!(
        custom,
        "--opaque=one --disable-features=opaque --remote-debugging-port=9323"
    );
    assert!(development_browser_arguments("0", "").is_err());
}
