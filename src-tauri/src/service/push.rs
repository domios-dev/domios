use crate::service::store::ServicesStore;
use crate::service::tokens::TokenStore;

pub fn extract_service_token(
    auth_header: &str,
    tokens: &TokenStore,
) -> std::io::Result<Option<String>> {
    let prefix = "Bearer ";
    if !auth_header.starts_with(prefix) {
        return Ok(None);
    }
    let token = &auth_header[prefix.len()..];
    if token.is_empty() {
        return Ok(None);
    }
    tokens.lookup_service_id(token)
}

pub fn validate_push(
    auth_header: &str,
    tokens: &TokenStore,
    store: &ServicesStore,
) -> std::io::Result<Option<String>> {
    let Some(svc_id) = extract_service_token(auth_header, tokens)? else {
        return Ok(None);
    };
    let Some(service) = store.get(&svc_id).ok().flatten() else {
        return Ok(None);
    };
    if service.enabled && service.push_enabled {
        Ok(Some(svc_id))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_service_token_extracts_service_id() {
        let dir = tempfile::tempdir().unwrap();
        let tokens = TokenStore::new_with_root(dir.path().to_path_buf());
        let token = tokens.generate("svc_test").unwrap();
        let auth_header = format!("Bearer {}", token);
        let result = extract_service_token(&auth_header, &tokens).unwrap();
        assert_eq!(result, Some("svc_test".to_string()));
    }

    #[test]
    fn missing_bearer_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let tokens = TokenStore::new_with_root(dir.path().to_path_buf());
        let result = extract_service_token("Bearer not-a-real-token", &tokens).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn malformed_header_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let tokens = TokenStore::new_with_root(dir.path().to_path_buf());
        assert_eq!(
            extract_service_token("not-bearer xxx", &tokens).unwrap(),
            None
        );
        assert_eq!(extract_service_token("", &tokens).unwrap(), None);
    }
}
