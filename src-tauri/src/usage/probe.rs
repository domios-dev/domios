use std::time::{Duration, Instant};

use crate::service::spawn::{AgentSpawnSpec, PaneCommand, PaneVisibility, SpawnPaneSpec};
use crate::service::ServiceContext;

#[derive(Debug, Clone)]
pub struct UsageProbeOutcome {
    pub raw_text: String,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone)]
pub enum ProbeError {
    BinaryMissing(String),
    NotReady(String),
    NoOutput,
    ExitTimeout,
    Internal(String),
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BinaryMissing(p) => write!(f, "binary missing: {}", p),
            Self::NotReady(m) => write!(f, "agent not ready: {}", m),
            Self::NoOutput => write!(f, "no /usage output captured"),
            Self::ExitTimeout => write!(f, "agent did not exit cleanly"),
            Self::Internal(m) => write!(f, "internal: {}", m),
        }
    }
}

impl std::error::Error for ProbeError {}

pub struct UsageProbe {
    svc: ServiceContext,
}

impl UsageProbe {
    pub fn new(svc: ServiceContext) -> Self {
        Self { svc }
    }

    pub async fn run(
        &self,
        provider: &str,
        _account_id: Option<&str>,
    ) -> Result<UsageProbeOutcome, ProbeError> {
        let started = Instant::now();
        let provider_owned = provider.to_string();
        let svc = self.svc.clone();

        let registry_has = svc
            .agent_registry
            .with_provider(&provider_owned, |_| ())
            .is_some();
        if !registry_has {
            return Err(ProbeError::BinaryMissing(provider_owned));
        }

        let svc_for_spawn = svc.clone();
        let provider_for_spawn = provider_owned.clone();
        let handle = tokio::task::spawn_blocking(move || {
            svc_for_spawn.spawn_pane(SpawnPaneSpec {
                workspace_id: None,
                parent_pane_id: None,
                cwd: None,
                title: Some(format!("[usage-probe] {}", provider_for_spawn)),
                visibility: PaneVisibility::Hidden,
                eager: false,
                command: PaneCommand::Agent(AgentSpawnSpec {
                    provider: provider_for_spawn,
                    auto_approve: true,
                    ..Default::default()
                }),
            })
        })
        .await
        .map_err(|e| ProbeError::Internal(format!("spawn join: {}", e)))?
        .map_err(|e| ProbeError::Internal(format!("spawn pane: {}", e)))?;

        let pane_id = handle.pane_id.clone();
        let pty_id = match handle.pty_id.clone() {
            Some(id) => id,
            None => {
                let _ = svc.close_pane(&pane_id);
                return Err(ProbeError::Internal("pane has no pty".into()));
            }
        };

        let outcome = self
            .probe_inner(&pane_id, &pty_id, &provider_owned, started)
            .await;

        let svc_for_close = svc.clone();
        let pane_for_close = pane_id.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let _ = svc_for_close.close_pane(&pane_for_close);
        })
        .await;

        outcome
    }

    async fn probe_inner(
        &self,
        pane_id: &str,
        pty_id: &str,
        provider: &str,
        started: Instant,
    ) -> Result<UsageProbeOutcome, ProbeError> {
        let svc = self.svc.clone();
        let warmup = svc
            .agent_registry
            .with_provider(provider, |p| p.task_input_warmup())
            .unwrap_or(Duration::ZERO);

        wait_for_tui(&svc, pty_id, TUI_READY_TIMEOUT)
            .await
            .map_err(ProbeError::NotReady)?;

        {
            let mut pty = svc.pty.lock();
            let _ = pty.resize(pty_id, 120, 200);
        }
        tokio::time::sleep(Duration::from_millis(800)).await;

        // ESC clears any partial input / open menu and warms the ConPTY input
        // path before the real command write.
        let pty_owned = pty_id.to_string();
        let svc_for_send = svc.clone();
        tokio::task::spawn_blocking(move || {
            let pty = svc_for_send.pty.lock();
            pty.write(&pty_owned, b"\x1b")
        })
        .await
        .map_err(|e| ProbeError::Internal(format!("esc join: {}", e)))?
        .map_err(|e| ProbeError::Internal(format!("send esc: {}", e)))?;
        tokio::time::sleep(Duration::from_millis(300)).await;

        let probe_cmd = svc
            .agent_registry
            .with_provider(provider, |p| p.usage_probe_command().map(|s| s.to_string()))
            .flatten()
            .unwrap_or_else(|| "/usage".to_string());

        write_slash_command(&svc, pty_id, &probe_cmd).await?;

        tokio::time::sleep(warmup).await;

        let raw_text = wait_for_output(&svc, pty_id, provider).await?;

        let _ = write_slash_command(&svc, pty_id, "/exit").await;

        let _ = pane_id;
        Ok(UsageProbeOutcome {
            raw_text,
            elapsed_ms: started.elapsed().as_millis() as u64,
        })
    }
}

/// Write a slash command into an already-running agent TUI as a CLEAN bracketed
/// paste, then submit.
///
/// Deliberately bypasses `send_text_verified`, which on Windows prepends a
/// sacrificial leading space (`\x1b[200~ {}\x1b[201~`) to absorb the
/// ConPTY/PSReadLine first-byte race. That race is a *shell-startup*
/// phenomenon (it gates the launch command, already handled upstream via
/// `poll_shell_prompt` in `spawn_hidden_agent_impl`). By the time we reach
/// here the agent TUI has been rendering for seconds, so no first byte is
/// dropped — and the leading space *survives*, turning `/usage` into ` /usage`
/// which the TUI treats as prose, not a command. That is the intermittent
/// Windows probe failure: a warmer ConPTY made the byte less likely to drop,
/// so the poisoning space appeared more often, not less. A clean paste keeps
/// the leading `/` intact on every platform.
async fn write_slash_command(
    svc: &ServiceContext,
    pty_id: &str,
    cmd: &str,
) -> Result<(), ProbeError> {
    let payload = format!("\x1b[200~{}\x1b[201~", cmd);
    let pty_owned = pty_id.to_string();
    let svc_for_paste = svc.clone();
    tokio::task::spawn_blocking(move || {
        let pty = svc_for_paste.pty.lock();
        pty.write(&pty_owned, payload.as_bytes())
    })
    .await
    .map_err(|e| ProbeError::Internal(format!("paste join: {}", e)))?
    .map_err(|e| ProbeError::Internal(format!("paste write: {}", e)))?;

    // Submit twice with a settle between: a freshly-rendered TUI can swallow a
    // too-early `\r`, leaving the command typed-but-never-sent.
    for settle_ms in [150u64, 600] {
        tokio::time::sleep(Duration::from_millis(settle_ms)).await;
        let pty_owned = pty_id.to_string();
        let svc_for_enter = svc.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let pty = svc_for_enter.pty.lock();
            pty.write(&pty_owned, b"\r")
        })
        .await;
    }
    Ok(())
}

async fn wait_for_tui(svc: &ServiceContext, pty_id: &str, timeout: Duration) -> Result<(), String> {
    let started = Instant::now();
    let poll = Duration::from_millis(250);
    let content_fallback_after = Duration::from_secs(25);
    let mut first_content_at: Option<Instant> = None;
    while started.elapsed() < timeout {
        let text = svc
            .screen_state
            .lock()
            .get_screen_text(pty_id)
            .unwrap_or_default();
        if screen_has_tui_input(&text) {
            return Ok(());
        }
        if !text.trim().is_empty() {
            let first = *first_content_at.get_or_insert_with(Instant::now);
            if first.elapsed() >= content_fallback_after {
                return Ok(());
            }
        }
        tokio::time::sleep(poll).await;
    }
    Err(format!(
        "TUI input box not detected after {}ms",
        timeout.as_millis()
    ))
}

fn screen_has_tui_input(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    text.contains("│ >")
        || text.contains("│  >")
        || text.contains("❯")
        || text.contains("│ Try")
        || lower.contains("try \"")
        || lower.contains("for shortcuts")
        || lower.contains("? for shortcuts")
        || lower.contains("ctrl+l")
}

/// How long to wait for the agent TUI input box to render before giving up.
/// Windows agent boot (node + provider) is meaningfully slower, so allow more
/// headroom; a genuine dead launch still fails honestly within this bound and
/// the poller retries with a fresh pane.
#[cfg(windows)]
const TUI_READY_TIMEOUT: Duration = Duration::from_secs(45);
#[cfg(not(windows))]
const TUI_READY_TIMEOUT: Duration = Duration::from_secs(30);

/// Total time to wait for the `/usage` panel to reach a parseable state. The
/// panel fetches usage data over the network and routinely shows
/// "Loading usage data…" for several seconds on a cold/slow connection — the
/// old 8s budget (plus a 5s loading-abort) frequently gave up before the data
/// landed, the core "does not fetch reliably" symptom. Windows gets extra
/// headroom for the slower render path.
#[cfg(windows)]
const TOTAL_BUDGET: Duration = Duration::from_secs(25);
#[cfg(not(windows))]
const TOTAL_BUDGET: Duration = Duration::from_secs(15);

const POLL_INTERVAL: Duration = Duration::from_millis(400);

async fn wait_for_output(
    svc: &ServiceContext,
    pty_id: &str,
    provider: &str,
) -> Result<String, ProbeError> {
    let started = Instant::now();
    let mut last_text = String::new();
    while started.elapsed() < TOTAL_BUDGET {
        let raw_bytes = {
            let ob = svc.output_buf.lock();
            ob.get_full(pty_id).map(|b| b.to_vec())
        };
        let text = match raw_bytes {
            Some(b) if !b.is_empty() => render_tall(&b, 200, 120),
            _ => svc
                .screen_state
                .lock()
                .get_screen_text(pty_id)
                .unwrap_or_default(),
        };
        if !text.trim().is_empty() {
            last_text = text.clone();
            if has_parseable_markers(&text, provider) {
                return Ok(text);
            }
            // "Loading usage data…" is normal progress, not a failure — keep
            // polling through the full budget instead of bailing early.
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    if last_text.trim().is_empty() {
        Err(ProbeError::NoOutput)
    } else if has_parseable_markers(&last_text, provider) {
        Ok(last_text)
    } else if only_loading_state(&last_text) {
        Err(ProbeError::NotReady(format!(
            "usage panel still loading after {}ms",
            TOTAL_BUDGET.as_millis()
        )))
    } else {
        Err(ProbeError::NotReady(
            "output never reached parseable state".into(),
        ))
    }
}

fn has_parseable_markers(text: &str, provider: &str) -> bool {
    match provider {
        "claude" => text.contains("% used") && text.contains("Resets"),
        "codex" => text.contains("% left") && text.contains("resets"),
        _ => false,
    }
}

fn only_loading_state(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("loading usage data")
        || lower.contains("loading…")
        || lower.contains("loading...")
}

fn render_tall(bytes: &[u8], rows: u16, cols: u16) -> String {
    let start = bytes.len().saturating_sub(200 * 1024);
    let mut parser = vt100::Parser::new(rows, cols, 0);
    parser.process(&bytes[start..]);
    parser.screen().contents()
}
