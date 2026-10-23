pub(crate) fn functions_url() -> Option<&'static str> {
    #[cfg(debug_assertions)]
    {
        option_env!("DOMIOS_BILLING_FUNCTIONS_URL").filter(|value| !value.trim().is_empty())
    }
    #[cfg(not(debug_assertions))]
    {
        Some(codeterm_account::firebase_endpoints::PRODUCTION.functions)
    }
}

#[cfg(all(test, not(debug_assertions)))]
mod tests {
    #[test]
    fn release_ignores_backend_overrides() {
        assert_eq!(
            super::functions_url(),
            Some(codeterm_account::firebase_endpoints::PRODUCTION.functions)
        );
        assert!(!codeterm_account::firebase_endpoints::current().emulator);
    }
}
