pub mod fake;
pub mod firebase;
pub mod local;
use crate::{Config, Error, IdentityProvider, Result};
use std::sync::Arc;
pub fn configured(
    config: &Config,
    installation_id: crate::InstallationId,
) -> Result<Arc<dyn IdentityProvider>> {
    let config = crate::firebase_endpoints::account_config(config.clone());
    let config = &config;
    match config.provider.as_str() {
        "local" => Ok(Arc::new(local::Local)),
        "fake" => Ok(Arc::new(fake::Fake::default())),
        "firebase" if firebase::available(config) => Ok(Arc::new(firebase::Firebase::new(
            config.clone(),
            Arc::new(firebase::HttpTransport::new()?),
            installation_id,
        ))),
        "firebase" => Ok(Arc::new(local::Local)),
        _ => Err(Error::Configuration),
    }
}

pub fn default_config() -> Config {
    firebase::default_config()
}
