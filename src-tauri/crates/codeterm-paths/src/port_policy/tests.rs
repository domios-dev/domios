use super::*;

#[test]
fn development_refuses_production_from_config_or_environment() {
    for (configured, environment) in [
        (7685, None),
        (7685, Some("7799")),
        (7686, Some("7685")),
        (7699, Some("07685")),
    ] {
        assert!(runtime_api_port(configured, environment, true).is_err());
    }
    assert!(validate_listener_port(7685, true).is_err());
}

#[test]
fn development_launch_port_overrides_saved_config() {
    assert_eq!(runtime_api_port(7686, Some("7799"), true), Ok(7799));
    assert_eq!(runtime_api_port(7686, None, true), Ok(7686));
    assert_eq!(runtime_api_port(7799, None, true), Ok(7799));
}

#[test]
fn invalid_environment_never_falls_back_to_saved_config() {
    for value in ["", "bad", "0", "65536"] {
        assert!(runtime_api_port(7686, Some(value), true).is_err());
    }
}

#[test]
fn release_uses_config_and_can_bind_production() {
    assert_eq!(runtime_api_port(7685, Some("7799"), false), Ok(7685));
    assert_eq!(runtime_api_port(7799, Some("invalid"), false), Ok(7799));
    assert!(validate_listener_port(7685, false).is_ok());
}
