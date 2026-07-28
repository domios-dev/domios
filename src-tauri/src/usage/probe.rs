use std::time::{Duration, Instant};

use crate::service::spawn::{AgentSpawnSpec, SpawnTabSpec, TabCommand, TabVisibility};
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

        let svc_for_fetch = svc.clone();
        let provider_for_fetch = provider_owned.clone();
        let fetched = tokio::task::spawn_blocking(move || {
            svc_for_fetch
                .agent_registry
                .with_provider(&provider_for_fetch, |p| p.fetch_usage())
                .flatten()
        })
        .await
        .map_err(|e| ProbeError::Internal(format!("fetch join: {}", e)))?;
        if let Some(result) = fetched {
            return result
                .map(|raw_text| UsageProbeOutcome {
                    raw_text,
                    elapsed_ms: started.elapsed().as_millis() as u64,
                })
                .map_err(ProbeError::NotReady);
        }

        let svc_for_spawn = svc.clone();
        let provider_for_spawn = provider_owned.clone();
        let handle = tokio::task::spawn_blocking(move || {
            svc_for_spawn.spawn_tab(SpawnTabSpec {
                workspace_id: None,
                parent_tab_id: None,
                cwd: None,
                title: Some(format!("[usage-probe] {}", provider_for_spawn)),
                description: None,
                visibility: TabVisibility::Hidden,
                eager: false,
                command: TabCommand::Agent(AgentSpawnSpec {
                    provider: provider_for_spawn,
                    auto_approve: true,
                    ..Default::default()
                }),
            })
        })
        .await
        .map_err(|e| ProbeError::Internal(format!("spawn join: {}", e)))?
        .map_err(|e| ProbeError::Internal(format!("spawn tab: {}", e)))?;

        let tab_id = handle.tab_id.clone();
        let pty_id = match handle.pty_id.clone() {
            Some(id) => id,
            None => {
                let _ = svc.close_tab(&tab_id);
                return Err(ProbeError::Internal("Tab has no pty".into()));
            }
        };

        let outcome = self
            .probe_inner(&tab_id, &pty_id, &provider_owned, started)
            .await;

        let svc_for_close = svc.clone();
        let tab_for_close = tab_id.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let _ = svc_for_close.close_tab(&tab_for_close);
        })
        .await;

        outcome
    }

    async fn probe_inner(
        &self,
        tab_id: &str,
        pty_id: &str,
        provider: &str,
        started: Instant,
    ) -> Result<UsageProbeOutcome, ProbeError> {
        let svc = self.svc.clone();
        let warmup = svc
            .agent_registry
            .with_provider(provider, |p| p.task_input_warmup())
            .unwrap_or(Duration::ZERO)
            .min(USAGE_PROBE_WARMUP_CAP);

        wait_for_tui(&svc, pty_id, provider, TUI_READY_TIMEOUT)
            .await
            .map_err(ProbeError::NotReady)?;

        {
            let mut pty = svc.pty.lock();
            let _ = pty.resize(pty_id, 120, 200);
        }
        tokio::time::sleep(Duration::from_millis(800)).await;

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

        let raw_text = probe_usage_command(&svc, pty_id, provider, &probe_cmd, warmup).await?;

        let _ = write_slash_command(&svc, pty_id, "/exit").await;

        let _ = tab_id;
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

/// Slash-command probes don't type long prompts — cap the post-command settle so
/// codex/claude don't burn 3s of task-delivery warmup on every `/status` poll.
const USAGE_PROBE_WARMUP_CAP: Duration = Duration::from_millis(500);

async fn wait_for_tui(
    svc: &ServiceContext,
    pty_id: &str,
    provider: &str,
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
        let provider_tui = svc
            .agent_registry
            .with_provider(provider, |p| p.screen_has_tui(&text))
            .unwrap_or(false);
        if screen_has_tui_input(&text) || provider_tui {
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
/// the poller retries with a fresh tab.
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
const TOTAL_BUDGET: Duration = Duration::from_secs(22);

const POLL_INTERVAL: Duration = Duration::from_millis(400);
const PROBE_COMMAND_ATTEMPTS: usize = 3;

async fn probe_usage_command(
    svc: &ServiceContext,
    pty_id: &str,
    provider: &str,
    probe_cmd: &str,
    warmup: Duration,
) -> Result<String, ProbeError> {
    let mut last_err = ProbeError::NoOutput;
    let min_accept_attempt = usize::from(probe_cmd.trim() == "/status");
    for attempt in 0..PROBE_COMMAND_ATTEMPTS {
        if attempt > 0 {
            clear_probe_screen(svc, pty_id).await?;
        }
        let baseline = OutputEpoch::capture(svc, pty_id);
        write_slash_command(svc, pty_id, probe_cmd).await?;
        tokio::time::sleep(warmup).await;
        if attempt < min_accept_attempt {
            wait_for_epoch_activity(svc, pty_id, &baseline, Duration::from_secs(3)).await;
            continue;
        }
        match wait_for_output_after(svc, pty_id, provider, &baseline).await {
            Ok(text) => return Ok(text),
            Err(err) => last_err = err,
        }
    }
    Err(last_err)
}

async fn clear_probe_screen(svc: &ServiceContext, pty_id: &str) -> Result<(), ProbeError> {
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
    Ok(())
}

async fn wait_for_epoch_activity(
    svc: &ServiceContext,
    pty_id: &str,
    baseline: &OutputEpoch,
    timeout: Duration,
) {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if !epoch_text(svc, pty_id, baseline).trim().is_empty() {
            return;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

#[derive(Debug, Clone)]
struct OutputEpoch {
    offset: u64,
    screen_hash: Option<[u8; 32]>,
}

impl OutputEpoch {
    fn capture(svc: &ServiceContext, pty_id: &str) -> Self {
        let offset = svc.output_buf.lock().total_written(pty_id);
        let screen_hash = svc.screen_state.lock().get_content_hash(pty_id, 0);
        Self {
            offset,
            screen_hash,
        }
    }
}

async fn wait_for_output_after(
    svc: &ServiceContext,
    pty_id: &str,
    provider: &str,
    baseline: &OutputEpoch,
) -> Result<String, ProbeError> {
    let started = Instant::now();
    let mut last_text = String::new();
    while started.elapsed() < TOTAL_BUDGET {
        let text = epoch_text(svc, pty_id, baseline);
        if !text.trim().is_empty() {
            last_text = text.clone();
            if output_is_parseable(&svc, provider, &text) {
                return Ok(text);
            }
            // polling through the full budget instead of bailing early.
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    if last_text.trim().is_empty() {
        Err(ProbeError::NoOutput)
    } else if output_is_parseable(&svc, provider, &last_text) {
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

fn epoch_text(svc: &ServiceContext, pty_id: &str, baseline: &OutputEpoch) -> String {
    let raw_bytes = {
        let ob = svc.output_buf.lock();
        let (bytes, _) = ob.read_since(pty_id, baseline.offset);
        bytes
    };
    let (screen_changed, screen) = {
        let ss = svc.screen_state.lock();
        (
            ss.get_content_hash(pty_id, 0) != baseline.screen_hash,
            ss.get_screen_text(pty_id),
        )
    };
    select_epoch_text(&raw_bytes, screen_changed, screen)
}

fn select_epoch_text(raw_bytes: &[u8], screen_changed: bool, screen: Option<String>) -> String {
    if !raw_bytes.is_empty() {
        let rendered = render_tall(raw_bytes, 200, 120);
        if !rendered.trim().is_empty() {
            return rendered;
        }
    }
    if screen_changed {
        screen.unwrap_or_default()
    } else {
        String::new()
    }
}

fn output_is_parseable(svc: &ServiceContext, provider: &str, text: &str) -> bool {
    svc.agent_registry
        .with_provider(provider, |p| parser_accepts(p.usage_parser(), text))
        .unwrap_or(false)
}

fn parser_accepts(
    parser: Option<&dyn crate::agent::usage_parser::UsageParser>,
    text: &str,
) -> bool {
    use crate::agent::usage_parser::UsageParseOutcome;

    parser
        .and_then(|up| up.parse_outcome(text).ok())
        .is_some_and(|outcome| match outcome {
            UsageParseOutcome::Parsed(s) => s.has_any_field(),
            UsageParseOutcome::Skip(_) => false,
        })
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

#[cfg(test)]
mod tests {
    use super::{parser_accepts, select_epoch_text};
    use crate::agent::usage_parser::{
        UsageParseError, UsageParseOutcome, UsageParser, UsageSnapshot,
    };

    #[test]
    fn parser_accepts_none_is_not_ready() {
        assert!(!parser_accepts(None, "anything with % used and Resets"));
    }

    struct StubParser;
    impl UsageParser for StubParser {
        fn parse(&self, raw_text: &str) -> Result<UsageSnapshot, UsageParseError> {
            if raw_text.contains("% used") {
                let mut s = UsageSnapshot::empty("stub", raw_text, 0);
                s.session_pct = Some(42.0);
                Ok(s)
            } else {
                Err(UsageParseError::NothingFound)
            }
        }
    }

    #[test]
    fn parser_accepts_tracks_field_presence() {
        assert!(parser_accepts(
            Some(&StubParser),
            "Current session 50% used"
        ));
        assert!(!parser_accepts(Some(&StubParser), "Loading usage data…"));
    }

    #[test]
    fn nothing_found_is_a_skip_outcome() {
        let outcome = StubParser.parse_outcome("Loading usage data…").unwrap();
        assert!(matches!(outcome, UsageParseOutcome::Skip(_)));
    }

    #[test]
    fn epoch_text_ignores_unchanged_stale_screen() {
        let stale = Some("Codex status\n5h limit 78% left (resets 23:00)".to_string());
        assert_eq!(select_epoch_text(&[], false, stale), "");
    }

    #[test]
    fn epoch_text_prefers_bytes_written_after_baseline() {
        let stale = Some("Codex status\n5h limit 78% left (resets 23:00)".to_string());
        let text = select_epoch_text(
            b"fresh status\n5h limit 64% left (resets 23:30)",
            false,
            stale,
        );
        assert!(text.contains("fresh status"));
        assert!(!text.contains("78% left"));
    }

    #[test]
    fn real_claude_parser_gates_like_old_markers() {
        let parser = crate::plugin::agent_claude::usage_tui::ClaudeTuiUsageParser;
        let fixture = include_str!("fixtures/usage_v2_1.txt");
        assert!(
            parser_accepts(Some(&parser), fixture),
            "captured /usage screen must be accepted"
        );
        assert!(
            !parser_accepts(Some(&parser), "Loading usage data…"),
            "loading screen must not be accepted"
        );
    }
}
