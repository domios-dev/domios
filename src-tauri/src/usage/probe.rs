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

        wait_for_tui(&svc, pty_id, Duration::from_secs(30))
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

        tokio::time::sleep(warmup + Duration::from_millis(5500)).await;

        let raw_bytes = {
            let ob = svc.output_buf.lock();
            ob.get_full(pty_id).map(|b| b.to_vec())
        };
        let raw_text = match raw_bytes {
            Some(bytes) if !bytes.is_empty() => render_tall(&bytes, 200, 120),
            _ => {
                let s = svc.screen_state.lock();
                s.get_screen_text(pty_id).ok_or(ProbeError::NoOutput)?
            }
        };
        if raw_text.trim().is_empty() {
            return Err(ProbeError::NoOutput);
        }

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
}

async fn wait_for_tui(
    svc: &ServiceContext,
    pty_id: &str,
    timeout: Duration,
) -> Result<(), String> {
    let started = Instant::now();
    let poll = Duration::from_millis(250);
    while started.elapsed() < timeout {
        let ready = {
            let s = svc.screen_state.lock();
            s.get_screen_text(pty_id)
                .map(|t| screen_has_tui_input(&t))
                .unwrap_or(false)
        };
        if ready {
            return Ok(());
        }
        tokio::time::sleep(poll).await;
    }
    Err(format!("TUI input box not detected after {}ms", timeout.as_millis()))
}

fn screen_has_tui_input(text: &str) -> bool {
    text.contains("│ >") || text.contains("> ") || text.contains("❯") || text.contains("│ Try")
}

fn render_tall(bytes: &[u8], rows: u16, cols: u16) -> String {
    let start = bytes.len().saturating_sub(200 * 1024);
    let mut parser = vt100::Parser::new(rows, cols, 0);
    parser.process(&bytes[start..]);
    parser.screen().contents()
}
