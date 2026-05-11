use std::time::{Duration, Instant};

use crate::agent::provider::VerifySubmitStrategy;
use crate::service::inject_verify::VerifyOpts;
use crate::service::spawn::{
    AgentSpawnSpec, PaneCommand, PaneVisibility, SpawnPaneSpec,
};
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

        let outcome = self.probe_inner(&pane_id, &pty_id, &provider_owned, started).await;

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
        let (warmup, submit_strategy) = svc
            .agent_registry
            .with_provider(provider, |p| (p.task_input_warmup(), p.verify_submit_strategy()))
            .unwrap_or((Duration::ZERO, VerifySubmitStrategy::None));

        wait_for_tui(&svc, pty_id, Duration::from_secs(60))
            .await
            .map_err(|m| ProbeError::NotReady(m))?;

        {
            let mut pty = svc.pty.lock();
            let _ = pty.resize(pty_id, 120, 200);
        }
        tokio::time::sleep(Duration::from_millis(800)).await;

        let opts = VerifyOpts {
            require_prompt_first: false,
            verify_echo: false,
            verify_submit: true,
            prompt_timeout: Duration::from_secs(5),
            echo_timeout: Duration::from_secs(3),
            submit_timeout: Duration::from_secs(5),
        };

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

        let pty_owned = pty_id.to_string();
        let svc_for_send = svc.clone();
        let strategy_for_send = submit_strategy.clone();
        let opts_for_send = opts.clone();
        let cmd_for_send = probe_cmd.clone();
        tokio::task::spawn_blocking(move || {
            svc_for_send.send_text_verified(
                &pty_owned,
                &cmd_for_send,
                true,
                strategy_for_send,
                opts_for_send,
            )
        })
        .await
        .map_err(|e| ProbeError::Internal(format!("send join: {}", e)))?
        .map_err(|e| ProbeError::Internal(format!("send {}: {}", probe_cmd, e)))?;

        tokio::time::sleep(Duration::from_millis(500)).await;
        let pty_owned = pty_id.to_string();
        let svc_for_extra = svc.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let pty = svc_for_extra.pty.lock();
            pty.write(&pty_owned, b"\r")
        })
        .await;

        tokio::time::sleep(warmup).await;

        let raw_text = wait_for_output(&svc, pty_id, provider).await?;

        let svc_for_exit = svc.clone();
        let pty_exit = pty_id.to_string();
        let _ = tokio::task::spawn_blocking(move || {
            svc_for_exit.send_text_verified(
                &pty_exit,
                "/exit",
                true,
                VerifySubmitStrategy::None,
                VerifyOpts {
                    require_prompt_first: false,
                    verify_echo: false,
                    verify_submit: false,
                    ..Default::default()
                },
            )
        })
        .await;

        let _ = pane_id;
        Ok(UsageProbeOutcome {
            raw_text,
            elapsed_ms: started.elapsed().as_millis() as u64,
        })
    }

    /// Read `/usage` from an already-running pane — no spawn, no close.
    /// Sends ESC + the probe command, captures output, then sends ESC again
    /// to dismiss the usage overlay and restore the pane's previous state.
    pub async fn run_in_pane(
        &self,
        pane_id: &str,
        pty_id: &str,
        provider: &str,
    ) -> Result<UsageProbeOutcome, ProbeError> {
        let started = Instant::now();
        let svc = self.svc.clone();
        let _ = pane_id;

        let probe_cmd = svc
            .agent_registry
            .with_provider(provider, |p| p.usage_probe_command().map(|s| s.to_string()))
            .flatten()
            .unwrap_or_else(|| "/usage".to_string());

        let submit_strategy = svc
            .agent_registry
            .with_provider(provider, |p| p.verify_submit_strategy())
            .unwrap_or(VerifySubmitStrategy::None);

        // ESC to clear any pending input or dismiss open panels
        let pty_owned = pty_id.to_string();
        let svc_esc = svc.clone();
        tokio::task::spawn_blocking(move || {
            let pty = svc_esc.pty.lock();
            pty.write(&pty_owned, b"\x1b")
        })
        .await
        .map_err(|e| ProbeError::Internal(format!("esc join: {}", e)))?
        .map_err(|e| ProbeError::Internal(format!("send esc: {}", e)))?;
        tokio::time::sleep(Duration::from_millis(300)).await;

        let pty_owned = pty_id.to_string();
        let svc_send = svc.clone();
        let cmd = probe_cmd.clone();
        tokio::task::spawn_blocking(move || {
            svc_send.send_text_verified(
                &pty_owned,
                &cmd,
                true,
                submit_strategy,
                VerifyOpts {
                    require_prompt_first: false,
                    verify_echo: false,
                    verify_submit: true,
                    prompt_timeout: Duration::from_secs(3),
                    echo_timeout: Duration::from_secs(3),
                    submit_timeout: Duration::from_secs(3),
                },
            )
        })
        .await
        .map_err(|e| ProbeError::Internal(format!("send join: {}", e)))?
        .map_err(|e| ProbeError::Internal(format!("send {}: {}", probe_cmd, e)))?;

        let raw_text = wait_for_output_budgeted(&svc, pty_id, provider, Duration::from_secs(5)).await?;

        // ESC to close the usage overlay and give the pane its input focus back
        let pty_owned = pty_id.to_string();
        let svc_restore = svc.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let pty = svc_restore.pty.lock();
            pty.write(&pty_owned, b"\x1b")
        })
        .await;

        Ok(UsageProbeOutcome {
            raw_text,
            elapsed_ms: started.elapsed().as_millis() as u64,
        })
    }
}

async fn wait_for_tui(
    svc: &ServiceContext,
    pty_id: &str,
    timeout: Duration,
) -> Result<(), String> {
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
    Err(format!("TUI input box not detected after {}ms", timeout.as_millis()))
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

const TOTAL_BUDGET: Duration = Duration::from_secs(8);
const POLL_INTERVAL: Duration = Duration::from_millis(400);

async fn wait_for_output(
    svc: &ServiceContext,
    pty_id: &str,
    provider: &str,
) -> Result<String, ProbeError> {
    wait_for_output_budgeted(svc, pty_id, provider, TOTAL_BUDGET).await
}

async fn wait_for_output_budgeted(
    svc: &ServiceContext,
    pty_id: &str,
    provider: &str,
    budget: Duration,
) -> Result<String, ProbeError> {
    let started = Instant::now();
    // If still in "loading" state past 5/8 of budget, bail early.
    let loading_deadline = budget * 5 / 8;
    let mut last_text = String::new();
    while started.elapsed() < budget {
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
            if started.elapsed() >= loading_deadline && only_loading_state(&text) {
                return Err(ProbeError::NotReady("still loading".into()));
            }
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    if last_text.trim().is_empty() {
        Err(ProbeError::NoOutput)
    } else if has_parseable_markers(&last_text, provider) {
        Ok(last_text)
    } else {
        Err(ProbeError::NotReady("output never reached parseable state".into()))
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
    lower.contains("loading usage data") || lower.contains("loading…") || lower.contains("loading...")
}

fn render_tall(bytes: &[u8], rows: u16, cols: u16) -> String {
    let start = bytes.len().saturating_sub(200 * 1024);
    let mut parser = vt100::Parser::new(rows, cols, 0);
    parser.process(&bytes[start..]);
    parser.screen().contents()
}
