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
    if root.starts_with("/usr/local/Cellar") {
        return Err(RuntimeFailure::new("runtime_unavailable"));
    }
    let framework_layout = root
        .components()
        .any(|part| part.as_os_str().to_string_lossy().ends_with(".framework"));
    let cellar_layout = root.components().any(|part| part.as_os_str() == "Cellar");
    if !framework_layout && !cellar_layout {
        return Err(RuntimeFailure::new("runtime_unavailable"));
    }
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
    output: Option<&codeterm_blueprint_types::blueprint_document::Port>,
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
        "body.mts"
    });
    std::fs::write(&body, code.src.as_deref().unwrap_or_default())
        .map_err(|_| RuntimeFailure::new("sandbox_denied"))?;
    let harness = temp.join(if code.lang == Language::Python {
        "harness.py"
    } else {
        "harness.mjs"
    });
    std::fs::write(
        &harness,
        if code.lang == Language::Python {
            include_str!("harness.py")
        } else {
            include_str!("harness.mjs")
        },
    )
    .map_err(|_| RuntimeFailure::new("sandbox_denied"))?;
    let output_metadata = serde_json::json!({"media":output.and_then(|p| p.media.as_deref()).unwrap_or("json"),
        "schema_id":output.and_then(|p| p.reference.as_deref()),"value":null,"text":""});
    std::fs::write(temp.join("output.json"), output_metadata.to_string())
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
    args.push(harness.into_os_string());
    let output = executable::run_limited(
        Path::new(SEATBELT),
        &args,
        &temp,
        timeout.saturating_sub(started.elapsed()),
        owner,
        cancellation,
        &Environment::isolated(&temp),
        input,
        None,
        code.limits.output_bytes.unwrap_or(32768).min(32768),
    )
    .map_err(|failure| RuntimeFailure {
        reason: if failure.primary == "budget_exhausted" {
            "budget_exhausted"
        } else if failure.primary == "probe_cancelled" {
            "skip:lease_cancelled"
        } else if failure.is_deadline() {
            "deadline"
        } else {
            "runtime_unavailable"
        },
        uncertain: failure.is_unconfirmed(),
    })?;
    let stderr = output.stderr_text();
    if stderr.lines().any(|line| {
        line == "DOMIOS_SANDBOX_DENIED"
            || line.starts_with("sandbox-exec: ")
            || line.starts_with("sandbox_apply:")
    }) {
        return Err(RuntimeFailure::new("sandbox_denied"));
    }
    if stderr.contains("DOMIOS_CONTRACT_VIOLATION") {
        return Err(RuntimeFailure::new("contract_violation"));
    }
    if code.lang == Language::Ts
        && (stderr.contains("bad option:") || stderr.contains("unknown option"))
    {
        return Err(RuntimeFailure::new("runtime_unavailable"));
    }
    directory
        .close()
        .map_err(|_| RuntimeFailure::new("sandbox_denied"))?;
    Ok(output)
}
