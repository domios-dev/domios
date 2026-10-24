use super::*;
use std::process::Stdio;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

async fn submit_code(input: &mut (impl AsyncWrite + Unpin), code: &str) -> Result<(), String> {
    async {
        input.write_all(code.as_bytes()).await?;
        input.write_all(b"\n").await?;
        input.flush().await
    }
    .await
    .map_err(|_| "Sign-in code could not be submitted".into())
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

pub(super) async fn run(
    svc: &ServiceContext,
    tab: &str,
    id: &str,
    request: Request,
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
        codeterm_plugin::js_runtime::probe_bin(binary).resolution
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
    #[cfg(windows)]
    let _job = crate::service::windows_job::Job::attach(&child)?;
    let mut input = child.stdin.take().ok_or("Sign-in stdin is unavailable")?;
    let stdout = child.stdout.take().ok_or("Sign-in stdout is unavailable")?;
    let stderr = child.stderr.take().ok_or("Sign-in stderr is unavailable")?;
    let outcome = {
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
                    let result = tokio::select! {
                        result = submit_code(&mut input, &code) => result,
                        _ = &mut deadline => Err("Sign-in timed out".into()),
                        _ = stopping.changed() => Err("Sign-in canceled".into()),
                    };
                    let failed = result.is_err();
                    let _ = receipt.send(result);
                    if failed { break Err("Sign-in code input ended before acknowledgement".into()); }
                }
            }
        }
    };
    if outcome.is_err()
        && child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_none()
    {
        child
            .kill()
            .await
            .map_err(|error| format!("Sign-in process cleanup failed: {error}"))?;
    }
    outcome
}

#[cfg(test)]
mod tests;
