use codeterm_service_core::service::account::deep_links;

pub fn register_deep_links(
    app: &codeterm_host_runtime::app_runtime::AppHandle,
    service: codeterm_service_core::service::ServiceState,
) -> Result<(), Box<dyn std::error::Error>> {
    use tauri_plugin_deep_link::DeepLinkExt;
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    app.deep_link().register_all()?;
    let dispatch = move |urls: Vec<url::Url>| {
        let service = service.clone();
        codeterm_service_core::async_runtime::spawn(async move {
            for url in urls {
                match deep_links::deep_link_action(&url) {
                    Some(deep_links::DeepLinkAction::BillingReturn { status }) => {
                        log::debug!(
                            "[billing] return received, status flag present: {}",
                            status.is_some()
                        );
                        let _ = service
                            .refresh_billing(codeterm_billing::RefreshEvent::CheckoutReturned)
                            .await;
                        let shell = service.shell.clone();
                        let _ = tokio::task::spawn_blocking(move || {
                            let _ = shell.focus_window("main");
                        })
                        .await;
                    }
                    Some(deep_links::DeepLinkAction::AccountCallback(callback)) => {
                        if let Ok(account) = service.account().await {
                            let _ = account.complete(callback).await;
                        }
                    }
                    None => {}
                }
            }
        });
    };
    if let Some(urls) = app.deep_link().get_current()? {
        dispatch(urls);
    }
    app.deep_link()
        .on_open_url(move |event| dispatch(event.urls()));
    Ok(())
}
