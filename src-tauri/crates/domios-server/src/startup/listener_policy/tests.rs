use super::*;

#[test]
fn isolated_verification_has_only_loopback_api_and_no_discovery() {
    for host in ["0.0.0.0", "::", "100.64.0.1"] {
        let mut config = AppConfig::default();
        config.api.enabled = false;
        config.api.host = host.into();
        config.api.port = 18765;
        config.api.mesh_tailnet_host = Some("100.64.0.2".into());
        config.api.cors = true;
        config.api.cors_allow_origins = vec!["https://example.com".into()];
        config.mesh.enabled = true;

        apply_isolated_listener_policy(&mut config);

        assert!(config.api.enabled);
        assert_eq!(
            codeterm_service_core::config::canonical_api_bind_addr(&config.api).unwrap(),
            "127.0.0.1:18765".parse().unwrap()
        );
        for debug in [false, true] {
            assert_eq!(
                codeterm_service_core::startup::rest_api_additional_bind(&config.api, debug),
                Ok(None)
            );
        }
        assert!(!config.mesh.enabled);
        assert!(!config.api.cors);
        assert!(config.api.cors_allow_origins.is_empty());
    }
}
