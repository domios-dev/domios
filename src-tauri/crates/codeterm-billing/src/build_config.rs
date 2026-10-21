pub(crate) fn functions_url() -> Option<&'static str> {
    option_env!("DOMIOS_BILLING_FUNCTIONS_URL").filter(|value| !value.trim().is_empty())
}
