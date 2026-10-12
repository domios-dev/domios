#[derive(Clone, Copy, Default)]
pub(crate) struct Vendor {
    pub api_key: Option<&'static str>,
    pub project_id: Option<&'static str>,
    pub app_id: Option<&'static str>,
    pub redeem_url: Option<&'static str>,
}
pub(crate) fn vendor() -> Vendor {
    Vendor {
        api_key: option_env!("DOMIOS_FIREBASE_API_KEY"),
        project_id: option_env!("DOMIOS_FIREBASE_PROJECT_ID"),
        app_id: option_env!("DOMIOS_FIREBASE_APP_ID"),
        redeem_url: option_env!("DOMIOS_ACCOUNT_REDEEM_URL"),
    }
}
