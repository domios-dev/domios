use super::*;

fn fixture() -> Config {
    serde_json::from_value(serde_json::json!({
        "identifier": "com.domios.app",
        "productName": "Opaque release product",
        "build": { "frontendDist": "../dist" },
        "app": { "windows": [{ "label": "main", "additionalBrowserArgs": "opaque-release" }] }
    }))
    .unwrap()
}

#[test]
fn release_and_non_development_configuration_are_byte_equivalent() {
    let development = InstanceSelector::Development {
        instance: Some("first".into()),
        port: 17801,
    };
    for (debug, selector) in [(false, development), (true, InstanceSelector::Production)] {
        let mut config = fixture();
        let before = serde_json::to_vec(&config).unwrap();
        configure(&mut config, debug, &selector, |_| {
            panic!("production read development environment")
        })
        .unwrap();
        assert_eq!(serde_json::to_vec(&config).unwrap(), before);
    }
}

#[test]
fn one_compiled_client_applies_distinct_runtime_instance_settings() {
    for (name, frontend, cdp) in [("first", "1821", "9321"), ("second", "1822", "9322")] {
        let mut config = fixture();
        let selector = InstanceSelector::Development {
            instance: Some(name.into()),
            port: 17801,
        };
        configure(&mut config, true, &selector, |key| match key {
            "CODETERM_VITE_PORT" => Some(frontend.into()),
            "CODETERM_DEV_IDENTIFIER" => Some(format!("com.domios.dev.{name}")),
            "CODETERM_CDP_PORT" => Some(cdp.into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(config.identifier, format!("com.domios.dev.{name}"));
        assert_eq!(
            config.build.dev_url.unwrap().as_str(),
            format!("http://127.0.0.1:{frontend}/")
        );
        assert!(config.app.windows[0]
            .additional_browser_args
            .as_ref()
            .unwrap()
            .contains(&format!("--remote-debugging-port={cdp}")));
    }
}

#[test]
fn invalid_runtime_identity_refuses_before_mutating_config() {
    let selector = InstanceSelector::Development {
        instance: Some("first".into()),
        port: 17801,
    };
    for (identifier, cdp) in [("com.domios.app", "9321"), ("com.domios.dev.first", "0")] {
        let mut config = fixture();
        let before = serde_json::to_vec(&config).unwrap();
        assert!(configure(&mut config, true, &selector, |key| match key {
            "CODETERM_VITE_PORT" => Some("1821".into()),
            "CODETERM_DEV_IDENTIFIER" => Some(identifier.into()),
            "CODETERM_CDP_PORT" => Some(cdp.into()),
            _ => None,
        })
        .is_err());
        assert_eq!(serde_json::to_vec(&config).unwrap(), before);
    }
}
