use codeterm_browser::PageChannel;
use serde_json::{json, Value};
use url::Url;

pub(super) const REFUSED_ORIGIN: &str =
    "The owner must allow this origin for the named profile in plugins.browser.blueprint_origins";

pub fn allows_page_origin(origins: &[String], target: &str) -> bool {
    Url::parse(target).is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && url.username().is_empty()
            && url.password().is_none()
            && origins.contains(&url.origin().ascii_serialization())
    })
}

pub(super) async fn main_frame(page: &PageChannel) -> Result<Value, String> {
    let tree = page
        .send("Page.getFrameTree", json!({}))
        .await
        .map_err(super::cdp_error)?;
    let frame = tree["frameTree"]["frame"].clone();
    if frame.is_object() {
        Ok(frame)
    } else {
        Err("browser frame unavailable".into())
    }
}

pub(super) async fn guard_navigation<T>(
    page: &PageChannel,
    origins: &[String],
    capture: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    let frame = main_frame(page).await?;
    let top = frame["id"]
        .as_str()
        .ok_or("browser frame identity missing")?;
    let mut events = page.subscribe_session(&["Fetch.requestPaused"]);
    page.send("Network.setBypassServiceWorker", json!({"bypass": true}))
        .await
        .map_err(super::cdp_error)?;
    page.send(
        "Fetch.enable",
        json!({"patterns":[{"resourceType":"Document"}]}),
    )
    .await
    .map_err(super::cdp_error)?;
    let intercept = async {
        while let Some(event) = events.recv().await {
            if events.has_lost_events() {
                return Err("browser lost origin enforcement events".into());
            }
            let params = &event.params;
            let request_id = params["requestId"]
                .as_str()
                .ok_or("browser request identity missing")?;
            let allowed = params["request"]["url"]
                .as_str()
                .is_some_and(|url| allows_page_origin(origins, url))
                && matches!(params["request"]["method"].as_str(), Some("GET" | "HEAD"));
            if allowed {
                page.send("Fetch.continueRequest", json!({"requestId":request_id}))
                    .await
                    .map_err(super::cdp_error)?;
            } else {
                page.send(
                    "Fetch.failRequest",
                    json!({"requestId":request_id,"errorReason":"BlockedByClient"}),
                )
                .await
                .map_err(super::cdp_error)?;
                if params["frameId"].as_str() == Some(top) {
                    return Err(REFUSED_ORIGIN.into());
                }
            }
        }
        Err("browser origin enforcement ended before capture".into())
    };
    tokio::select! {
        biased;
        denied = intercept => denied,
        result = capture => result,
    }
}

#[cfg(test)]
mod tests;
