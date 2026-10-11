use crate::frontend_bundle::FrontendBundles;
use crate::frontend_bundle_api_version::{API_VERSION, FRONTEND_PUBLIC_KEY};
use std::sync::Arc;

const DISCOVERY_URL: &str = "https://github.com/rollacode/codeterm-releases/releases/download/frontend-latest/frontend-manifest.json";
pub(crate) const BUNDLED_INDEX: &str = "/bundled/index.html";

pub(crate) fn open(
    events: &crate::api::events::EventBroadcasterState,
) -> Option<Arc<FrontendBundles>> {
    let events = events.clone();
    match FrontendBundles::open(
        &crate::paths::data_dir().join("frontend-bundles"),
        API_VERSION,
        FRONTEND_PUBLIC_KEY.map(str::to_owned),
        DISCOVERY_URL.into(),
        false,
        Arc::new(move |name, data| events.publish_generic(name, data)),
    ) {
        Ok(bundles) => Some(Arc::new(bundles)),
        Err(error) => {
            log::error!("[frontend-bundle] store unavailable: {error}");
            None
        }
    }
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
