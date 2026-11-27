use super::*;
use std::process::Stdio;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use zeroize::Zeroizing;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum CodeWriteOutcome {
    Written,
    FailedBeforeWrite(String),
    Uncertain(String),
}

async fn submit_code(
    input: &mut (impl AsyncWrite + Unpin),
    code: &Zeroizing<String>,
) -> CodeWriteOutcome {
    let mut payload = Zeroizing::new(Vec::with_capacity(code.len() + 1));
    payload.extend_from_slice(code.as_bytes());
    payload.push(b'\n');
    let mut written = 0usize;
    while written < payload.len() {
        match input.write(&payload[written..]).await {
            Ok(0) if written == 0 => {
                return CodeWriteOutcome::FailedBeforeWrite(
                    "Sign-in code input closed before accepting the code".into(),
                );
            }
            Ok(0) => {
                return CodeWriteOutcome::Uncertain(
                    "Sign-in code input closed mid-write; delivery is uncertain and was not replayed"
                        .into(),
                );
            }
            Ok(count) => written += count,
            Err(_) if written == 0 => {
                return CodeWriteOutcome::FailedBeforeWrite(
                    "Sign-in code could not be written to provider stdin".into(),
                );
            }
            Err(_) => {
                return CodeWriteOutcome::Uncertain(
                    "Sign-in code write ended after bytes were sent; delivery is uncertain and was not replayed"
                        .into(),
                );
            }
        }
    }
    match input.flush().await {
        Ok(()) => CodeWriteOutcome::Written,
        Err(_) => CodeWriteOutcome::Uncertain(
            "Sign-in code flush was not confirmed; delivery is uncertain and was not replayed"
                .into(),
        ),
    }
}

async fn capture(
    reader: impl AsyncRead + Unpin,
    mut publish: impl FnMut(String),
    pattern: &regex::Regex,
) -> Result<(), String> {
    let mut reader = reader;
    let mut bytes = [0u8; 2048];
    let mut captured = Vec::new();
    loop {
        let count = reader
            .read(&mut bytes)
            .await
            .map_err(|error| error.to_string())?;
        if count == 0 {
            return Ok(());
        }
        if captured.len() + count > 65_536 {
            return Err("Sign-in output exceeded its limit".into());
        }
        captured.extend_from_slice(&bytes[..count]);
        let text = String::from_utf8_lossy(&captured);
        if let Some(url) = pattern.captures(&text).and_then(|captures| captures.get(1)) {
            if let Ok(parsed) = url::Url::parse(url.as_str()) {
                if parsed.scheme() == "https"
                    && parsed.username().is_empty()
                    && parsed.password().is_none()
                {
                    publish(url.as_str().to_owned());
                }
            }
        }
    }
}

pub(super) struct Request {
    pub(super) provider: Arc<dyn AgentProvider>,
    pub(super) capability: CliLoginCapability,
    pub(super) args: Vec<String>,
    pub(super) stop: watch::Receiver<bool>,
    pub(super) codes: mpsc::Receiver<CodeSubmission>,
}

pub(super) struct RunResult {
    pub outcome: Result<(), String>,
    pub cleanup_confirmed: bool,
}

pub(super) async fn run(svc: &ServiceContext, tab: &str, id: &str, request: Request) -> RunResult {
    let mut cleanup_confirmed = true;
    let outcome = run_inner(svc, tab, id, request, &mut cleanup_confirmed).await;
    RunResult {
        outcome,
        cleanup_confirmed,
    }
}

async fn run_inner(
    svc: &ServiceContext,
    tab: &str,
    id: &str,
    request: Request,
    cleanup_confirmed: &mut bool,
) -> Result<(), String> {
    let Request {
        provider,
        capability: cap,
        args,
        stop: mut stopping,
        mut codes,
    } = request;
    if stopping.has_changed().is_err() || *stopping.borrow() {
        return Err("Sign-in canceled".into());
    }
    let pattern = regex::Regex::new(&cap.url_pattern)
        .map_err(|_| "Provider has an invalid sign-in link declaration")?;
    let binary = provider
        .cli_binary()
        .ok_or("Provider has no sign-in binary")?;
    let codeterm_plugin::js_runtime::BinResolution::Found(binary) =
        provider.probe_cli_binary(binary).resolution
    else {
        return Err("Sign-in executable is unavailable".into());
    };
    let mut command =
        tokio::process::Command::from(codeterm_plugin::provider_command::command(&binary)?);
    #[cfg(windows)]
    crate::process_util::no_window_tokio_suspended(&mut command);
    command
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    let _lease = crate::service::agent_runtime::lease::Lease::configure(&mut command)
        .map_err(|error| error.to_string())?;
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    *cleanup_confirmed = false;
    let outcome = async {
    #[cfg(windows)]
    let _job = crate::service::windows_job::Job::attach(&child)?;
    let mut input = child.stdin.take().ok_or("Sign-in stdin is unavailable")?;
    let stdout = child.stdout.take().ok_or("Sign-in stdout is unavailable")?;
    let stderr = child.stderr.take().ok_or("Sign-in stderr is unavailable")?;
    {
        let read = async {
            tokio::try_join!(
                capture(stdout, |url| observe(svc, tab, id, url), &pattern),
                capture(stderr, |url| observe(svc, tab, id, url), &pattern)
            )?;
            Ok::<_, String>(())
        };
        let operation = async {
            let (_, status) = tokio::try_join!(read, async {
                child.wait().await.map_err(|error| error.to_string())
            })?;
            if !status.success() {
                return Err(format!(
                    "Provider sign-in exited with code {:?}",
                    status.code()
                ));
            }
            Ok(())
        };
        tokio::pin!(operation);
        let deadline =
            tokio::time::sleep(Duration::from_millis(cap.timeout_ms.clamp(1000, 900_000)));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                result = &mut operation => break result,
                _ = &mut deadline => break Err("Sign-in timed out. Try again.".into()),
                _ = stopping.changed() => break Err("Sign-in canceled".into()),
                Some((code, receipt)) = codes.recv() => {
                    codes.close();
                    enum StopReason {
                        TimedOut,
                        Canceled,
                        None,
                    }
                    let (result, stop_reason) = tokio::select! {
                        result = submit_code(&mut input, &code) => (result, StopReason::None),
                        _ = &mut deadline => (
                            CodeWriteOutcome::Uncertain(
                                "Sign-in timed out while delivering the code; delivery is uncertain and was not replayed".into(),
                            ),
                            StopReason::TimedOut,
                        ),
                        _ = stopping.changed() => (
                            CodeWriteOutcome::Uncertain(
                                "Sign-in canceled while delivering the code; delivery is uncertain and was not replayed".into(),
                            ),
                            StopReason::Canceled,
                        ),
                    };
                    drop(code);
                    let failed_before_write = matches!(result, CodeWriteOutcome::FailedBeforeWrite(_));
                    let _ = receipt.send(result);
                    match stop_reason {
                        StopReason::TimedOut => break Err("Sign-in timed out. Try again.".into()),
                        StopReason::Canceled => break Err("Sign-in canceled".into()),
                        StopReason::None if failed_before_write => {
                            break Err("Sign-in code input ended before acknowledgement".into())
                        }
                        StopReason::None => {}
                    }
                }
            }
        }
    }
    }.await;
    if child
        .try_wait()
        .map_err(|error| error.to_string())?
        .is_none()
    {
        child
            .kill()
            .await
            .map_err(|error| format!("Sign-in process cleanup failed: {error}"))?;
    }
    *cleanup_confirmed = true;
    outcome
}

#[cfg(test)]
mod tests;
