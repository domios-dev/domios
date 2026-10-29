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
    let program = program.to_string();
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        runtime.block_on(async {
            use tokio::io::AsyncReadExt;
            let mut command = tokio::process::Command::new(&program);
            command
                .args(&args)
                .kill_on_drop(true)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            super::no_window_tokio(&mut command);
            let mut child = command.spawn()?;
            let mut stdout = child
                .stdout
                .take()
                .ok_or_else(|| io::Error::other("missing stdout pipe"))?
                .take(4 * 1024 * 1024 + 1);
            let mut stderr = child
                .stderr
                .take()
                .ok_or_else(|| io::Error::other("missing stderr pipe"))?
                .take(8193);
            let mut out = Vec::new();
            let mut err = Vec::new();
            let result = tokio::time::timeout(timeout, async {
                tokio::try_join!(
                    child.wait(),
                    stdout.read_to_end(&mut out),
                    stderr.read_to_end(&mut err)
                )
            })
            .await
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("timeout after {timeout:?}"),
                )
            })??;
            capture_result(result.0.code(), result.0.success(), &out, &err)
        })
    })
    .join()
    .map_err(|_| io::Error::other("probe worker panicked"))?
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
