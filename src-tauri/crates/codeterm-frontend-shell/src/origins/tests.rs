use super::*;

#[test]
fn frontend_shell_contract_uses_only_loopback_http_origins() {
    for (address, expected) in [
        ("127.0.0.1:7686", "http://127.0.0.1:7686"),
        ("0.0.0.0:7686", "http://127.0.0.1:7686"),
        ("[::]:7686", "http://[::1]:7686"),
        ("[::1]:7686", "http://[::1]:7686"),
    ] {
        assert_eq!(loopback_origin(address.parse().unwrap()).unwrap(), expected);
    }
    assert!(loopback_origin("192.168.1.2:7686".parse().unwrap()).is_err());
}

#[test]
fn frontend_shell_contract_release_ignores_development_url() {
    for target in ["http://127.0.0.1:1421", "https://foreign.example"] {
        assert_eq!(
            development_frontend(false, Some(&Url::parse(target).unwrap())).unwrap(),
            None
        );
    }
}

#[test]
fn frontend_shell_contract_debug_selects_configured_loopback_development_url() {
    assert_eq!(development_frontend(true, None).unwrap(), None);
    for target in [
        "http://127.0.0.1:1421/",
        "http://localhost:1422/",
        "http://[::1]:1423/",
    ] {
        let url = Url::parse(target).unwrap();
        assert_eq!(development_frontend(true, Some(&url)).unwrap(), Some(url));
    }
}

#[test]
fn frontend_shell_contract_refuses_untrusted_development_url() {
    for target in [
        "https://foreign.example",
        "http://192.168.1.2:1420",
        "http://user@127.0.0.1:1420",
        "file:///index.html",
    ] {
        assert!(development_frontend(true, Some(&Url::parse(target).unwrap())).is_err());
    }
}

#[test]
fn frontend_shell_contract_declared_origins_follow_the_listener() {
    let origins = declared_frontend_origins(Some("0.0.0.0:7690".parse().unwrap()), None);
    assert_eq!(
        origins,
        vec![
            fallback_origin().to_string(),
            "http://127.0.0.1:7690".into()
        ]
    );
    assert_eq!(
        declared_frontend_origins(None, None),
        vec![fallback_origin().to_string()]
    );
}
