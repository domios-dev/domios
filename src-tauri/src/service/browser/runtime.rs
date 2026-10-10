use super::cdp::{CdpClient, PageChannel, CONNECT_DEADLINE};
use super::cdp_wire::{headful_user_agent, parse_targets, pick_page_target};
use super::launch::{launch_args, validate_loopback_endpoint, LaunchSpec};
use super::locator::locate;
use super::process::{BrowserProcess, CloseOutcome, CLOSE_DEADLINE, KILL_DEADLINE, READY_DEADLINE};
use super::profile::{ProfileKind, ProfileLayout, ProfileLease};
use super::screencast::{FrameHub, ScreencastDriver};
use super::viewport::Viewport;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BrowserMode {
    Launched { profile: ProfileKind },
    Attached { http_base: String },
}

impl BrowserMode {
    pub fn profile_label(&self) -> &str {
        match self {
            Self::Launched { profile } => profile.label(),
            Self::Attached { .. } => "attached",
        }
    }
}

/// One browser connection: a launched or attached Chromium, its CDP socket and tracked page.
pub struct Runtime {
    pub mode: BrowserMode,
    pub endpoint: String,
    pub binary: Option<PathBuf>,
    pub process: Option<BrowserProcess>,
    pub profile_dir: Option<PathBuf>,
    pub client: CdpClient,
    pub page: PageChannel,
    pub window_id: Option<i64>,
    lease: Mutex<Option<ProfileLease>>,
    _screencast: ScreencastDriver,
}

impl Runtime {
    pub fn is_alive(&self) -> bool {
        self.client.close_reason().is_none()
            && self
                .process
                .as_ref()
                .is_none_or(|process| process.exit_status().is_none())
    }
}

pub(super) async fn launch(
    configured_binary: Option<PathBuf>,
    layout: &ProfileLayout,
    profile: ProfileKind,
    viewport: Viewport,
    frames: Arc<FrameHub>,
) -> Result<Runtime, String> {
    let located = tokio::task::spawn_blocking(move || locate(configured_binary.as_deref()))
        .await
        .map_err(|error| format!("browser lookup task failed: {error}"))?
        .map_err(|error| error.to_string())?;
    let profile_dir = layout.dir_for(&profile);
    let lease = ProfileLease::acquire(profile_dir.clone())?;
    tokio::fs::create_dir_all(&profile_dir)
        .await
        .map_err(|error| format!("create profile {}: {error}", profile_dir.display()))?;
    let args = launch_args(&LaunchSpec {
        profile_dir: &profile_dir,
        width: viewport.width,
        height: viewport.height,
        initial_url: "about:blank",
    });
    let process = match BrowserProcess::spawn(&located.binary, &args, READY_DEADLINE).await {
        Ok(process) => process,
        Err(error) => {
            discard_profile(&profile, &profile_dir).await;
            return Err(error);
        }
    };
    log::info!(
        "[browser] {} ready in {} ms, profile {}",
        located.binary.display(),
        process.ready_after().as_millis(),
        profile.label()
    );
    let endpoint = process.endpoint().to_string();
    let connected = connect(&endpoint, Some(viewport)).await;
    let (client, page, window_id) = match connected {
        Ok(connected) => connected,
        Err(error) => {
            process.kill();
            process
                .wait_exit_until(Instant::now() + KILL_DEADLINE)
                .await;
            discard_profile(&profile, &profile_dir).await;
            return Err(error);
        }
    };
    let screencast = ScreencastDriver::spawn(page.clone(), frames);
    Ok(Runtime {
        mode: BrowserMode::Launched { profile },
        endpoint,
        binary: Some(located.binary),
        process: Some(process),
        profile_dir: Some(profile_dir),
        client,
        page,
        window_id,
        lease: Mutex::new(Some(lease)),
        _screencast: screencast,
    })
}

pub(super) async fn attach(http_base: &str, frames: Arc<FrameHub>) -> Result<Runtime, String> {
    validate_loopback_endpoint(http_base, &["http"])?;
    let version_url = format!("{}/json/version", http_base.trim_end_matches('/'));
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(CONNECT_DEADLINE)
        .build()
        .map_err(|error| format!("http client: {error}"))?;
    let body: Value = http
        .get(&version_url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| format!("GET {version_url}: {error}"))?
        .json()
        .await
        .map_err(|error| format!("GET {version_url}: {error}"))?;
    let endpoint = version_endpoint(&body)?;
    let (client, page, window_id) = connect(&endpoint, None).await?;
    let screencast = ScreencastDriver::spawn(page.clone(), frames);
    Ok(Runtime {
        mode: BrowserMode::Attached {
            http_base: http_base.to_string(),
        },
        endpoint,
        binary: None,
        process: None,
        profile_dir: None,
        client,
        page,
        window_id,
        lease: Mutex::new(None),
        _screencast: screencast,
    })
}

/// `/json/version` body → the browser WebSocket endpoint, loopback only.
pub fn version_endpoint(body: &Value) -> Result<String, String> {
    let endpoint = body
        .get("webSocketDebuggerUrl")
        .and_then(Value::as_str)
        .ok_or("/json/version has no webSocketDebuggerUrl")?;
    validate_loopback_endpoint(endpoint, &["ws"])?;
    Ok(endpoint.to_string())
}

/// `owned_viewport` is `None` for an attached app, whose window and emulation we never touch.
async fn connect(
    endpoint: &str,
    owned_viewport: Option<Viewport>,
) -> Result<(CdpClient, PageChannel, Option<i64>), String> {
    let client = CdpClient::connect(endpoint).await?;
    let prepared = prepare(&client, owned_viewport).await;
    if prepared.is_err() {
        client.close("page setup failed");
    }
    prepared.map(|(page, window_id)| (client, page, window_id))
}

async fn prepare(
    client: &CdpClient,
    owned_viewport: Option<Viewport>,
) -> Result<(PageChannel, Option<i64>), String> {
    let cdp = |error: super::cdp::CdpError| error.to_string();
    let targets = parse_targets(
        &client
            .send("Target.getTargets", json!({}))
            .await
            .map_err(cdp)?,
    );
    let target_id = match pick_page_target(&targets) {
        Some(target) => target.target_id.clone(),
        None => client
            .send("Target.createTarget", json!({ "url": "about:blank" }))
            .await
            .map_err(cdp)?
            .get("targetId")
            .and_then(Value::as_str)
            .ok_or("Target.createTarget returned no targetId")?
            .to_string(),
    };
    let page = client.attach_page(&target_id).await.map_err(cdp)?;
    page.send("Page.enable", json!({})).await.map_err(cdp)?;
    client
        .send("Target.setDiscoverTargets", json!({ "discover": true }))
        .await
        .map_err(cdp)?;
    let Some(viewport) = owned_viewport else {
        return Ok((page, None));
    };
    let version = client
        .send("Browser.getVersion", json!({}))
        .await
        .map_err(cdp)?;
    if let Some(user_agent) = version.get("userAgent").and_then(Value::as_str) {
        page.send(
            "Network.setUserAgentOverride",
            json!({ "userAgent": headful_user_agent(user_agent), "acceptLanguage": "en-US,en" }),
        )
        .await
        .map_err(cdp)?;
    }
    let window_id = client
        .send(
            "Browser.getWindowForTarget",
            json!({ "targetId": target_id }),
        )
        .await
        .ok()
        .and_then(|result| result.get("windowId").and_then(Value::as_i64));
    apply_viewport(client, &page, window_id, viewport).await?;
    Ok((page, window_id))
}

pub(super) async fn apply_viewport(
    client: &CdpClient,
    page: &PageChannel,
    window_id: Option<i64>,
    viewport: Viewport,
) -> Result<(), String> {
    if let Some(window_id) = window_id {
        let bounds = json!({ "windowId": window_id, "bounds": { "width": viewport.width, "height": viewport.height } });
        if let Err(error) = client.send("Browser.setWindowBounds", bounds).await {
            log::warn!("[browser] setWindowBounds failed: {error}");
        }
    }
    page.send(
        "Emulation.setDeviceMetricsOverride",
        json!({
            "width": viewport.width,
            "height": viewport.height,
            "deviceScaleFactor": viewport.dpr,
            "mobile": false,
        }),
    )
    .await
    .map(|_| ())
    .map_err(|error| error.to_string())
}

/// The URL worth reopening after a profile switch: only real web pages, never about:/data:/devtools.
pub fn carry_url(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    matches!(parsed.scheme(), "http" | "https").then(|| url.to_string())
}

pub(super) async fn current_url(runtime: &Runtime) -> Option<String> {
    runtime
        .client
        .send(
            "Target.getTargetInfo",
            json!({ "targetId": runtime.page.target_id() }),
        )
        .await
        .ok()?
        .get("targetInfo")?
        .get("url")?
        .as_str()
        .map(str::to_string)
}

/// Graceful close flushes cookies into a named profile; attach mode only disconnects.
pub(super) async fn stop(runtime: Arc<Runtime>) -> Option<CloseOutcome> {
    stop_within(runtime, CLOSE_DEADLINE).await
}

/// `stop` with the graceful window bounded by `deadline` before the tree is killed.
pub(super) async fn stop_within(runtime: Arc<Runtime>, deadline: Duration) -> Option<CloseOutcome> {
    let outcome = match runtime.process.as_ref() {
        Some(process) => {
            let client = runtime.client.clone();
            let request_close = async move {
                let _ = client.send("Browser.close", json!({})).await;
            };
            Some(process.close_with(request_close, deadline).await)
        }
        None => None,
    };
    runtime.client.close("browser closed by host");
    if let (BrowserMode::Launched { profile }, Some(dir)) = (&runtime.mode, &runtime.profile_dir) {
        discard_profile(profile, dir).await;
    }
    drop(runtime.lease.lock().take());
    outcome
}

async fn discard_profile(profile: &ProfileKind, dir: &Path) {
    if profile.is_persistent() {
        return;
    }
    if let Err(error) = tokio::fs::remove_dir_all(dir).await {
        log::warn!(
            "[browser] throwaway profile {} not removed now, the startup sweep will: {error}",
            dir.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_runtime_version_endpoint_requires_loopback_ws() {
        assert_eq!(
            version_endpoint(&json!({
                "Browser": "Edg/141.0.0.0",
                "webSocketDebuggerUrl": "ws://127.0.0.1:9223/devtools/browser/0b4e"
            })),
            Ok("ws://127.0.0.1:9223/devtools/browser/0b4e".to_string())
        );
        assert!(
            version_endpoint(&json!({"webSocketDebuggerUrl": "ws://10.0.0.2:9223/x"})).is_err()
        );
        assert!(version_endpoint(&json!({"Browser": "Chrome"})).is_err());
    }

    #[test]
    fn browser_runtime_carries_only_web_pages_across_profile_switch() {
        assert_eq!(
            carry_url("https://mail.example.com/inbox?x=1"),
            Some("https://mail.example.com/inbox?x=1".into())
        );
        assert_eq!(
            carry_url("http://127.0.0.1:5173/"),
            Some("http://127.0.0.1:5173/".into())
        );
        assert_eq!(carry_url("about:blank"), None);
        assert_eq!(carry_url("chrome-error://chromewebdata/"), None);
        assert_eq!(carry_url("data:text/html,hi"), None);
        assert_eq!(carry_url(""), None);
    }

    #[test]
    fn browser_runtime_profile_label_names_attach_mode() {
        let attached = BrowserMode::Attached {
            http_base: "http://127.0.0.1:9223".into(),
        };
        assert_eq!(attached.profile_label(), "attached");
        let named = BrowserMode::Launched {
            profile: ProfileKind::Named {
                name: "work".into(),
            },
        };
        assert_eq!(named.profile_label(), "work");
    }
}
