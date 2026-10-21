use url::Url;

pub fn declared_frontend_origins(
    config: &crate::config::schema::ApiConfig,
    dev_url: Option<&Url>,
) -> Vec<String> {
    codeterm_frontend_shell::declared_frontend_origins(
        crate::config::canonical_api_bind_addr(config).ok(),
        dev_url,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontend_shell_contract_cors_origins_match_declared_navigation_origins() {
        let mut config = crate::config::schema::ApiConfig::default();
        config.port = 7690;
        let dev = Url::parse("http://127.0.0.1:1463/").unwrap();
        let origins = declared_frontend_origins(&config, Some(&dev));
        assert!(origins.contains(&codeterm_frontend_shell::fallback_origin().to_string()));
        assert!(origins.contains(&"http://127.0.0.1:7690".into()));
        assert_eq!(
            origins.contains(&"http://127.0.0.1:1463".into()),
            cfg!(debug_assertions)
        );
        assert!(!origins.contains(&"http://localhost:1463".into()));
        assert!(!origins.contains(&"https://tauri.localhost".into()));
    }
}
