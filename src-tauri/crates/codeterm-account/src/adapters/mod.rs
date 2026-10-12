pub mod fake;
pub mod firebase;
pub mod local;
use crate::{Config, Error, IdentityProvider, Result};
use std::sync::Arc;
pub fn configured(config: &Config) -> Result<Arc<dyn IdentityProvider>> {
    match config.provider.as_str() {
        "local" => Ok(Arc::new(local::Local)),
        "fake" => Ok(Arc::new(fake::Fake::default())),
        "firebase" => Ok(Arc::new(firebase::Firebase::new(
            config.clone(),
            Arc::new(firebase::HttpTransport::new()?),
        ))),
        _ => Err(Error::Configuration),
    }
}

pub fn default_config() -> Config {
    let (signin_url, redeem_url, api_key) = firebase::defaults();
    Config {
        provider: "local".into(),
        signin_url,
        redeem_url,
        api_key,
        login_timeout_secs: 180,
    }
}
