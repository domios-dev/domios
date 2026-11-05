use std::io;
use std::time::Duration;

#[derive(Debug)]
pub struct CaptureExit {
    pub code: Option<i32>,
    pub stderr: String,
}

impl std::fmt::Display for CaptureExit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "exit {:?}: {}", self.code, self.stderr)
    }
}

impl std::error::Error for CaptureExit {}

pub fn bounded_capture(program: &str, args: &[&str], timeout: Duration) -> io::Result<String> {
    let mut command = std::process::Command::new(program);
    command.args(args);
    super::no_window_std(&mut command);
    let output = bounded_output(command, timeout)?;
    capture_result(
        output.status.code(),
        output.status.success(),
        &output.stdout,
        &output.stderr,
    )
}

pub fn bounded_output(
    command: std::process::Command,
    timeout: Duration,
) -> io::Result<std::process::Output> {
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        runtime.block_on(async {
            let mut command = tokio::process::Command::from(command);
            command
                .kill_on_drop(true)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            super::no_window_tokio(&mut command);
            let mut child = command.spawn()?;
            let mut stdout = child
                .stdout
                .take()
                .ok_or_else(|| io::Error::other("missing stdout pipe"))?;
            let mut stderr = child
                .stderr
                .take()
                .ok_or_else(|| io::Error::other("missing stderr pipe"))?;
            let mut out = Vec::new();
            let mut err = Vec::new();
            let result = tokio::time::timeout(timeout, async {
                tokio::try_join!(
                    child.wait(),
                    read_limited(&mut stdout, &mut out, 4 * 1024 * 1024),
                    read_limited(&mut stderr, &mut err, 8192)
                )
            })
            .await
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("timeout after {timeout:?}"),
                )
            })
            .and_then(|result| result);
            match result {
                Ok((status, _, _)) => Ok(std::process::Output {
                    status,
                    stdout: out,
                    stderr: err,
                }),
                Err(error) => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    Err(error)
                }
            }
        })
    })
    .join()
    .map_err(|_| io::Error::other("probe worker panicked"))?
}

async fn read_limited(
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
    output: &mut Vec<u8>,
    limit: u64,
) -> io::Result<()> {
    use tokio::io::AsyncReadExt;
    reader.take(limit + 1).read_to_end(output).await?;
    if output.len() as u64 > limit {
        return Err(io::Error::other("output limit exceeded"));
    }
    Ok(())
}

fn capture_result(code: Option<i32>, success: bool, out: &[u8], err: &[u8]) -> io::Result<String> {
    if out.len() > 4 * 1024 * 1024 || err.len() > 8192 {
        return Err(io::Error::other("output limit exceeded"));
    }
    if !success {
        return Err(io::Error::other(CaptureExit {
            code,
            stderr: String::from_utf8_lossy(err).trim().to_string(),
        }));
    }
    Ok(String::from_utf8_lossy(out).into_owned())
}

#[cfg(test)]
mod tests;
