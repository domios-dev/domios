use super::*;
use crate::probe::executable;
use std::ffi::OsString;

pub(super) fn run(
    code: &Code,
    input: &[u8],
    limit: Duration,
    runtime: Duration,
    owner: &Arc<ProbeResourceOwner>,
    cancellation: &Cancellation,
    output: Option<&codeterm_blueprint_types::blueprint_document::Port>,
    cwd: &std::path::Path,
) -> Result<Output, RuntimeFailure> {
    execute(
        code,
        input,
        limit,
        runtime,
        owner,
        cancellation,
        output,
        &crate::probe_env::command(),
        cwd,
    )
}

pub(super) fn execute(
    code: &Code,
    input: &[u8],
    limit: Duration,
    runtime: Duration,
    owner: &Arc<ProbeResourceOwner>,
    cancellation: &Cancellation,
    output: Option<&codeterm_blueprint_types::blueprint_document::Port>,
    environment: &crate::probe::environment::Environment,
    cwd: &std::path::Path,
) -> Result<Output, RuntimeFailure> {
    let started = std::time::Instant::now();
    let names: &[&str] = match code.lang {
        Language::Python => {
            if cfg!(windows) {
                &["python3.exe", "python.exe"]
            } else {
                &["python3", "python"]
            }
        }
        Language::Ts => {
            if cfg!(windows) {
                &["node.exe"]
            } else {
                &["node"]
            }
        }
        Language::Shell => return Err(RuntimeFailure::new("contract_violation")),
    };
    let interpreter = environment
        .resolve_program(names)
        .ok_or_else(|| RuntimeFailure::new("runtime_unavailable"))?;
    let directory = tempfile::tempdir().map_err(|_| RuntimeFailure::new("runtime_unavailable"))?;
    let temp = directory.path();
    let (body, harness, content, mut args) = match code.lang {
        Language::Python => (
            "body.py",
            "harness.py",
            include_str!("harness.py"),
            vec!["-I".into(), "-S".into(), "-B".into()],
        ),
        Language::Ts => (
            "body.mts",
            "harness.mjs",
            include_str!("harness.mjs"),
            vec!["--experimental-strip-types".into()],
        ),
        Language::Shell => return Err(RuntimeFailure::new("contract_violation")),
    };
    let metadata = serde_json::json!({"media":output.and_then(|p| p.media.as_deref()).unwrap_or("json"),
        "schema_id":output.and_then(|p| p.reference.as_deref()),"value":null,"text":""});
    for (file, content) in [
        (body, code.src.as_deref().unwrap_or_default()),
        (harness, content),
        ("output.json", &metadata.to_string()),
    ] {
        std::fs::write(temp.join(file), content)
            .map_err(|_| RuntimeFailure::new("runtime_unavailable"))?;
    }
    args.push(OsString::from(temp.join(harness)));
    let memory = code
        .limits
        .mem_mb
        .map(|mb| {
            mb.checked_mul(1024 * 1024)
                .filter(|bytes| *bytes > 0)
                .ok_or_else(|| RuntimeFailure::new("contract_violation"))
        })
        .transpose()?;
    let result = executable::run_limited(
        &interpreter,
        &args,
        cwd,
        limit.saturating_sub(started.elapsed()),
        runtime,
        owner,
        cancellation,
        environment,
        input,
        memory,
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
        #[cfg(any(test, feature = "test-support"))]
        stderr_tail: failure.stderr_tail,
    })?;
    let stderr = result.stderr_text();
    if stderr.contains("DOMIOS_CONTRACT_VIOLATION") {
        return Err(RuntimeFailure::observed("contract_violation", &stderr));
    }
    if code.lang == Language::Ts
        && (stderr.contains("bad option:") || stderr.contains("unknown option"))
    {
        return Err(RuntimeFailure::observed("runtime_unavailable", &stderr));
    }
    directory
        .close()
        .map_err(|_| RuntimeFailure::new("runtime_unavailable"))?;
    Ok(result)
}
