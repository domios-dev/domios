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
