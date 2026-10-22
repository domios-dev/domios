use super::*;
use crate::probe::{environment::Environment, executable};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

const SEATBELT: &str = "/usr/bin/sandbox-exec";

fn interpreter(language: &Language) -> Result<PathBuf, RuntimeFailure> {
    let paths: &[&str] = match language {
        Language::Python => &[
            "/usr/local/bin/python3",
            "/opt/homebrew/bin/python3",
            "/usr/bin/python3",
        ],
        Language::Ts => &[
            "/opt/homebrew/bin/node",
            "/usr/local/bin/node",
            "/usr/bin/node",
        ],
        _ => return Err(RuntimeFailure::new("runtime_unavailable")),
    };
    paths
        .iter()
        .find_map(|path| std::fs::canonicalize(path).ok())
        .ok_or_else(|| RuntimeFailure::new("runtime_unavailable"))
}

fn quoted(path: &Path) -> Result<String, RuntimeFailure> {
    serde_json::to_string(
        path.to_str()
            .ok_or_else(|| RuntimeFailure::new("sandbox_denied"))?,
    )
    .map_err(|_| RuntimeFailure::new("sandbox_denied"))
}

fn profile(interpreter: &Path, temp: &Path) -> Result<String, RuntimeFailure> {
    let root = interpreter
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| RuntimeFailure::new("sandbox_denied"))?;
    Ok(format!(
        r#"(version 1)
(deny default)
(allow process-exec (literal {}))
(allow process-info* (target self))
(allow signal (target self))
(allow sysctl-read)
(allow file-read-metadata)
(allow file-read* (subpath "/System/Library") (subpath "/usr/lib")
 (subpath {}) (subpath {}) (literal "/dev/null") (literal "/dev/urandom")
 (regex #"^/opt/homebrew/Cellar/[^/]+/[^/]+/lib/[^/]+[.]dylib$"))
(allow file-write* (subpath {}) (literal "/dev/null"))
"#,
        quoted(interpreter)?,
        quoted(root)?,
        quoted(temp)?,
        quoted(temp)?
    ))
}

pub(super) fn run(
    code: &Code,
    input: &[u8],
    timeout: Duration,
    owner: &Arc<ProbeResourceOwner>,
    cancellation: &Cancellation,
) -> Result<Output, RuntimeFailure> {
    let started = std::time::Instant::now();
    if !Path::new(SEATBELT).is_file() {
        return Err(RuntimeFailure::new("sandbox_denied"));
    }
    let interpreter = interpreter(&code.lang)?;
    let directory = tempfile::tempdir().map_err(|_| RuntimeFailure::new("sandbox_denied"))?;
    let temp = directory
        .path()
        .canonicalize()
        .map_err(|_| RuntimeFailure::new("sandbox_denied"))?;
    let profile_path = temp.join("confinement.sb");
    std::fs::write(&profile_path, profile(&interpreter, &temp)?)
        .map_err(|_| RuntimeFailure::new("sandbox_denied"))?;
    let body = temp.join(if code.lang == Language::Python {
        "body.py"
    } else {
        "body.ts"
    });
    std::fs::write(&body, code.src.as_deref().unwrap_or_default())
        .map_err(|_| RuntimeFailure::new("sandbox_denied"))?;
    let mut args = vec![
        OsString::from("-f"),
        profile_path.into_os_string(),
        interpreter.into_os_string(),
    ];
    match code.lang {
        Language::Python => args.extend(["-I".into(), "-S".into(), "-B".into()]),
        Language::Ts => args.extend([
            "--disable-code-generation-from-strings".into(),
            "--experimental-strip-types".into(),
        ]),
        _ => return Err(RuntimeFailure::new("runtime_unavailable")),
    }
    args.push(body.into_os_string());
    let output = executable::run(
        Path::new(SEATBELT),
        &args,
        &temp,
        timeout.saturating_sub(started.elapsed()),
        owner,
        cancellation,
        &Environment::isolated(&temp),
        input,
    )
    .map_err(|failure| RuntimeFailure {
        reason: if failure.is_deadline() {
            "deadline"
        } else {
            "sandbox_denied"
        },
        uncertain: failure.is_unconfirmed(),
    })?;
    directory
        .close()
        .map_err(|_| RuntimeFailure::new("sandbox_denied"))?;
    Ok(output)
}
