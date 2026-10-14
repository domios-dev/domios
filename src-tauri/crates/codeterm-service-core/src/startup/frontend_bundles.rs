use crate::frontend_bundle_api_version::{
    API_VERSION, BUNDLED_FRONTEND_VERSION, FRONTEND_PUBLIC_KEY,
};
use codeterm_frontend_bundle::FrontendBundles;
use std::sync::Arc;

const DISCOVERY_URL: &str = "https://github.com/rollacode/codeterm-releases/releases/download/frontend-latest/frontend-manifest.json";
pub(crate) const BUNDLED_INDEX: &str = "/bundled/index.html";

pub(crate) fn open(
    events: &codeterm_events::EventBroadcasterState,
) -> Option<Arc<FrontendBundles>> {
    let events = events.clone();
    let (discovery, public_key, loopback) = dev_discovery_override().unwrap_or_else(|| {
        (
            DISCOVERY_URL.into(),
            FRONTEND_PUBLIC_KEY.map(str::to_owned),
            false,
        )
    });
    match FrontendBundles::open(
        &crate::paths::data_dir().join("frontend-bundles"),
        API_VERSION,
        BUNDLED_FRONTEND_VERSION,
        public_key,
        discovery,
        loopback,
        Arc::new(move |name, data| events.publish_generic(name, data)),
    ) {
        Ok(bundles) => Some(Arc::new(bundles)),
        Err(error) => {
            log::error!("[frontend-bundle] store unavailable: {error}");
            None
        }
    }
}

// DEV-only frontend swap drill (evals/frontend-swap): a loopback discovery URL and a test key.
#[cfg(debug_assertions)]
fn dev_discovery_override() -> Option<(String, Option<String>, bool)> {
    let discovery = std::env::var("CODETERM_DEV_FRONTEND_DISCOVERY_URL").ok()?;
    let public_key = std::env::var("CODETERM_DEV_FRONTEND_PUBLIC_KEY").ok()?;
    log::warn!("[frontend-bundle] DEV discovery override: {discovery}");
    Some((discovery, Some(public_key), true))
}

#[cfg(not(debug_assertions))]
fn dev_discovery_override() -> Option<(String, Option<String>, bool)> {
    None
}

pub(crate) fn navigation_index(bundles: Option<&FrontendBundles>) -> String {
    bundles
        .and_then(FrontendBundles::active_index)
        .unwrap_or_else(|| BUNDLED_INDEX.into())
}

pub(crate) fn check(bundles: Arc<FrontendBundles>) {
    tauri::async_runtime::spawn(async move {
        match tokio::task::spawn_blocking(move || bundles.check()).await {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => log::debug!("[frontend-bundle] check refused: {error}"),
            Err(error) => log::warn!("[frontend-bundle] check worker failed: {error}"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontend_bundle_routes_contract_bundled_navigation_without_store() {
        assert_eq!(navigation_index(None), "/bundled/index.html");
    }
}

#[cfg(test)]
mod signing_tests {
    use super::*;

    #[test]
    fn frontend_bundle_integrity_unprovisioned_runtime_fails_closed() {
        let root = tempfile::tempdir().unwrap();
        let bundles = FrontendBundles::open(
            root.path(),
            API_VERSION,
            "0.0.0",
            None,
            DISCOVERY_URL.into(),
            false,
            Arc::new(|_, _| {}),
        )
        .unwrap();
        assert_eq!(bundles.status()["updatesEnabled"], false);
        assert_eq!(
            bundles.check().unwrap_err(),
            "Frontend signing identity not provisioned"
        );
        assert_eq!(navigation_index(Some(&bundles)), BUNDLED_INDEX);
    }
}
