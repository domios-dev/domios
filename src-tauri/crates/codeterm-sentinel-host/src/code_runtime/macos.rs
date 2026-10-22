use super::*;
use crate::probe::{environment::Environment, executable};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

const SEATBELT: &str = "/usr/bin/sandbox-exec";

fn interpreter(language: &Language) -> Result<(PathBuf, PathBuf), RuntimeFailure> {
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
    let binary = paths
        .iter()
        .find_map(|path| std::fs::canonicalize(path).ok())
        .ok_or_else(|| RuntimeFailure::new("runtime_unavailable"))?;
    let root = binary
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| RuntimeFailure::new("runtime_unavailable"))?
        .to_path_buf();
    let framework = root.join("Resources/Python.app/Contents/MacOS/Python");
    let program = if *language == Language::Python && framework.is_file() {
        framework
            .canonicalize()
            .map_err(|_| RuntimeFailure::new("runtime_unavailable"))?
    } else {
        binary
    };
    Ok((program, root))
}

fn quoted(path: &Path) -> Result<String, RuntimeFailure> {
    serde_json::to_string(
        path.to_str()
            .ok_or_else(|| RuntimeFailure::new("sandbox_denied"))?,
    )
    .map_err(|_| RuntimeFailure::new("sandbox_denied"))
}

fn profile(interpreter: &Path, root: &Path, temp: &Path) -> Result<String, RuntimeFailure> {
    Ok(format!(
        r#"(version 1)
(deny default)
(allow process-exec (literal {}))
(allow process-info* (target self))
(allow signal (target self))
(allow sysctl-read)
(allow file-read-metadata)
(allow file-read* (literal "/"))
(allow file-read* file-map-executable (subpath "/System/Library") (subpath "/usr/lib")
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
    let (interpreter, runtime_root) = interpreter(&code.lang)?;
    let directory = tempfile::tempdir().map_err(|_| RuntimeFailure::new("sandbox_denied"))?;
    let temp = directory
        .path()
        .canonicalize()
        .map_err(|_| RuntimeFailure::new("sandbox_denied"))?;
    let profile_path = temp.join("confinement.sb");
    std::fs::write(&profile_path, profile(&interpreter, &runtime_root, &temp)?)
        .map_err(|_| RuntimeFailure::new("sandbox_denied"))?;
    std::fs::write(temp.join("openssl.cnf"), "")
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
            "--disallow-code-generation-from-strings".into(),
            "--experimental-strip-types".into(),
        ]),
        _ => return Err(RuntimeFailure::new("runtime_unavailable")),
    }
    if code.lang == Language::Ts {
        let mut capability_args = args.clone();
        capability_args.push("--version".into());
        let capability = executable::run(
            Path::new(SEATBELT),
            &capability_args,
            &temp,
            timeout.saturating_sub(started.elapsed()),
            owner,
            cancellation,
            &Environment::isolated(&temp),
            b"",
        )
        .map_err(|failure| RuntimeFailure {
            reason: if failure.is_deadline() {
                "deadline"
            } else {
                "sandbox_denied"
            },
            uncertain: failure.is_unconfirmed(),
        })?;
        if !capability.success || !capability.stdout_text().starts_with('v') {
            return Err(RuntimeFailure::new("runtime_unavailable"));
        }
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
