use super::*;

#[test]
fn agent_task_wait_observes_cancellation_without_waiting_for_job_completion() {
    let (release, held) = std::sync::mpsc::channel();
    let id = start_gated_job("cancel-fixture", move || {
        let _ = held.recv();
    });
    let cancellation = crate::converter::ConversionCancellation::default();
    cancellation.cancel();
    assert!(matches!(
        wait_json_interruptible(
            "cancel-fixture",
            &id,
            &cancellation,
            Instant::now() + Duration::from_secs(10)
        ),
        WaitResult::TimedOut
    ));
    release.send(()).unwrap();
    assert!(
        serde_json::from_str::<serde_json::Value>(&poll("cancel-fixture", &id)).unwrap()["error"]
            .is_string()
    );
}

#[test]
fn exec_output_retains_only_the_bounded_prefix() {
    let bytes = vec![b'x'; 256 * 1024];
    let mut reader = std::io::Cursor::new(bytes);
    assert_eq!(bounded_output(&mut reader).len(), 128 * 1024);
    assert_eq!(reader.position(), 256 * 1024);
}
use crate::js_runtime::host_api::PluginSandbox;
use crate::js_runtime::PROCESS_TEST_LOCK as TEST_LOCK;
use std::time::Instant;

fn reset_exec_jobs() {
    EXEC_JOBS.lock().clear();
}

fn print_hello_opts() -> serde_json::Value {
    #[cfg(windows)]
    {
        serde_json::json!({
            "bin": "powershell",
            "args": ["-NoProfile", "-Command", "[Console]::Out.Write('hello')"],
        })
    }
    #[cfg(not(windows))]
    {
        serde_json::json!({ "bin": "sh", "args": ["-c", "printf hello"] })
    }
}

fn sleep_opts() -> serde_json::Value {
    #[cfg(windows)]
    {
        serde_json::json!({
            "bin": "powershell",
            "args": ["-NoProfile", "-Command", "Start-Sleep -Seconds 2"],
        })
    }
    #[cfg(not(windows))]
    {
        serde_json::json!({ "bin": "sh", "args": ["-c", "sleep 2"] })
    }
}

fn poll_until_done(plugin_id: &str, job_id: &str) -> serde_json::Value {
    let started = Instant::now();
    loop {
        let v: serde_json::Value = serde_json::from_str(&poll(plugin_id, job_id)).unwrap();
        if v["done"].as_bool() == Some(true) {
            return v;
        }
        // A bounded failure deadline, not a performance budget: five seconds
        // measured machine load and failed under verify.
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "exec timed out"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn start_then_poll_yields_done_and_stdout() {
    let _guard = TEST_LOCK.lock();
    reset_exec_jobs();
    let started: serde_json::Value =
        serde_json::from_str(&start("test-plugin", &print_hello_opts().to_string())).unwrap();
    let job_id = started["jobId"].as_str().expect("job id");
    assert!(job_id.starts_with("exec:test-plugin:"));

    let done = poll_until_done("test-plugin", job_id);
    assert_eq!(done["done"], true);
    assert_eq!(done["code"], 0);
    assert_eq!(done["stdout"], "hello");

    let missing: serde_json::Value = serde_json::from_str(&poll("test-plugin", job_id)).unwrap();
    assert_eq!(missing["error"], "job not found");
}

#[test]
fn non_allowlisted_cmd_is_denied_by_same_gate_as_blocking_exec() {
    let _guard = TEST_LOCK.lock();
    reset_exec_jobs();
    let sb = PluginSandbox::restricted(vec![], vec![]);
    let opts = serde_json::json!({ "bin": "rm", "args": ["-rf", "/"] }).to_string();
    let bin = bin_of(&opts).unwrap_or_default();

    let raw = if sb.can_exec(&bin) {
        start("test-plugin", &opts)
    } else {
        err(&format!("exec denied: {bin}"))
    };

    let denied: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(denied["error"], "exec denied: rm");
}

#[test]
fn detached_exec_rejects_log_paths_outside_plugin_write_roots() {
    let _guard = TEST_LOCK.lock();
    reset_exec_jobs();
    let temp = tempfile::tempdir().unwrap();
    let allowed = temp.path().join("allowed");
    std::fs::create_dir_all(&allowed).unwrap();
    let outside = temp.path().join("outside.log");
    let outside_path = outside.to_string_lossy().into_owned();
    let sandbox = PluginSandbox::restricted(vec![], vec![allowed]);
    let opts = serde_json::json!({
        "bin": "missing-executable",
        "detach": true,
        "logFile": outside_path,
    });
    let opts_json = opts.to_string();
    let expected = format!(
        "detached exec log denied: {} is outside plugin write roots",
        outside.display()
    );

    let result: serde_json::Value =
        serde_json::from_str(&run_with_host_env(&opts_json, &HashMap::new(), &sandbox)).unwrap();
    let async_result: serde_json::Value = serde_json::from_str(&start_with_host_env(
        "log-denial-plugin",
        &opts_json,
        &HashMap::new(),
        Arc::new(PluginSandbox::restricted(
            vec![],
            vec![temp.path().join("allowed")],
        )),
    ))
    .unwrap();

    assert_eq!(result["error"].as_str(), Some(expected.as_str()));
    assert_eq!(async_result["error"].as_str(), Some(expected.as_str()));
    assert!(EXEC_JOBS.lock().is_empty());
    assert!(!outside.exists());
}

#[test]
fn host_exec_env_overrides_plugin_env_for_identity() {
    let opts = serde_json::json!({
        "bin": "codeterm",
        "env": {
            "CODETERM_TAB_ID": "spoofed",
            "PLUGIN_VAR": "kept"
        }
    })
    .to_string();
    let host_env = HashMap::from([
        ("CODETERM_TAB_ID".to_string(), "chat-tab".to_string()),
        (
            "CODETERM_SOCKET_PATH".to_string(),
            "/tmp/codeterm.sock".to_string(),
        ),
    ]);

    let merged: ExecOpts =
        serde_json::from_str(&inject_host_env_json(&opts, &host_env).unwrap()).unwrap();

    assert_eq!(
        merged.env.get("CODETERM_TAB_ID").map(String::as_str),
        Some("chat-tab")
    );
    assert_eq!(
        merged.env.get("CODETERM_SOCKET_PATH").map(String::as_str),
        Some("/tmp/codeterm.sock")
    );
    assert_eq!(
        merged.env.get("PLUGIN_VAR").map(String::as_str),
        Some("kept")
    );
}

#[test]
fn start_returns_immediately_without_holding_the_caller_for_the_command_duration() {
    let _guard = TEST_LOCK.lock();
    reset_exec_jobs();
    let t = Instant::now();
    let started: serde_json::Value =
        serde_json::from_str(&start("slow-plugin", &sleep_opts().to_string())).unwrap();
    let elapsed = t.elapsed();
    let job_id = started["jobId"].as_str().expect("job id");

    assert!(
        elapsed < Duration::from_millis(500),
        "start blocked for {elapsed:?} — VM lock would be held for the exec duration"
    );

    let poll1: serde_json::Value = serde_json::from_str(&poll("slow-plugin", job_id)).unwrap();
    assert_eq!(poll1["done"], false);

    close("slow-plugin", job_id);
}

#[test]
fn exec_job_cap_is_per_plugin() {
    let _guard = TEST_LOCK.lock();
    reset_exec_jobs();
    for _ in 0..MAX_EXEC_JOBS_PER_PLUGIN {
        start_exec_job("cap-plugin").unwrap();
    }
    assert_eq!(
        start_exec_job("cap-plugin").unwrap_err(),
        format!("plugin cap-plugin reached the exec job limit ({MAX_EXEC_JOBS_PER_PLUGIN})")
    );
    let other_plugin_job = start_exec_job("other-plugin").unwrap();
    close("other-plugin", &other_plugin_job);
    drop_plugin_jobs("cap-plugin");
}

#[test]
fn completed_job_survives_an_unrelated_start_until_polled() {
    let _guard = TEST_LOCK.lock();
    reset_exec_jobs();
    let completed = start_exec_job("owner-plugin").unwrap();
    finish_exec_job(
        &completed,
        &serde_json::json!({ "code": 0, "stdout": "owned", "stderr": "" }),
    );

    let unrelated = start_exec_job("other-plugin").unwrap();
    let result: serde_json::Value =
        serde_json::from_str(&poll("owner-plugin", &completed)).unwrap();

    assert_eq!(result["code"], 0);
    assert_eq!(result["stdout"], "owned");
    close("other-plugin", &unrelated);
}

#[test]
fn insertion_reaps_completed_unpolled_jobs_past_retention() {
    let _guard = TEST_LOCK.lock();
    reset_exec_jobs();
    let completed = start_exec_job("leaky-plugin").unwrap();
    finish_exec_job(
        &completed,
        &serde_json::json!({ "code": 0, "stdout": "done", "stderr": "" }),
    );
    EXEC_JOBS.lock().get_mut(&completed).unwrap().finished_at =
        Some(Instant::now() - EXEC_JOB_RETENTION - Duration::from_millis(1));

    let next = start_exec_job("leaky-plugin").unwrap();
    let result: serde_json::Value =
        serde_json::from_str(&poll("leaky-plugin", &completed)).unwrap();

    assert_eq!(result["error"], "job not found");
    close("leaky-plugin", &next);
}

#[test]
fn run_drains_output_larger_than_pipe_buffer_without_deadlock() {
    #[cfg(windows)]
    let opts = serde_json::json!({
        "bin": "powershell",
        "args": ["-NoProfile", "-Command", "[Console]::Out.Write('x' * 200000)"],
        "timeoutMs": 10000,
    });
    #[cfg(not(windows))]
    let opts = serde_json::json!({
        "bin": "sh",
        "args": ["-c", "head -c 200000 /dev/zero | tr '\\0' x"],
        "timeoutMs": 10000,
    });

    // A deadlocked pipe cannot reach the assertions below, which is the proof.
    // Wall-clock is not: it measured machine load and flaked at 6.6s.
    let out: serde_json::Value = serde_json::from_str(&run(&opts.to_string())).unwrap();
    assert!(out.get("error").is_none(), "unexpected error: {out}");
    assert_eq!(out["code"], 0);
    assert_eq!(out["stdout"].as_str().map(str::len), Some(200_000));
}

#[test]
fn conversation_keeps_stdin_open_until_each_response_arrives() {
    #[cfg(windows)]
    let opts = serde_json::json!({
        "bin": "powershell",
        "args": ["-NoProfile", "-Command", r#"$line = $null; while (($line = [Console]::In.ReadLine()) -ne $null) { if ($line.Contains('"id":1')) { [Console]::Out.WriteLine('{"id":1,"result":{}}') }; if ($line.Contains('"id":2')) { [Console]::Out.WriteLine('{"id":2,"result":{}}') } }"#],
        "conversation": [
            { "stdin": "{\"method\":\"initialize\",\"id\":1}\n", "awaitResponseIds": [1] },
            { "stdin": "{\"method\":\"hooks/list\",\"id\":2}\n", "awaitResponseIds": [2] }
        ],
        "timeoutMs": 10000
    });
    #[cfg(not(windows))]
    let opts = serde_json::json!({
        "bin": "sh",
        "args": ["-c", r#"while IFS= read -r line; do case "$line" in *'"id":1'*) printf '%s\n' '{"id":1,"result":{}}';; *'"id":2'*) printf '%s\n' '{"id":2,"result":{}}';; esac; done"#],
        "conversation": [
            { "stdin": "{\"method\":\"initialize\",\"id\":1}\n", "awaitResponseIds": [1] },
            { "stdin": "{\"method\":\"hooks/list\",\"id\":2}\n", "awaitResponseIds": [2] }
        ],
        "timeoutMs": 10000
    });

    let out: serde_json::Value = serde_json::from_str(&run(&opts.to_string())).unwrap();
    assert_eq!(
        out["code"], 0,
        "conversation process must exit cleanly: {out}"
    );
    assert!(
        out.get("error").is_none(),
        "conversation must complete: {out}"
    );
    let stdout = out["stdout"].as_str().unwrap_or_default();
    assert!(stdout.contains("\"id\":1"));
    assert!(stdout.contains("\"id\":2"));
}

#[test]
fn run_times_out_blocking_process() {
    // The grandchild outsleeps any PowerShell startup jitter, so run()
    // blocking on it reads as a 30s outlier, not a boundary value.
    #[cfg(windows)]
    let opts = serde_json::json!({
        "bin": "powershell",
        "args": [
            "-NoProfile",
            "-Command",
            r#"$p = Start-Process -FilePath powershell -ArgumentList @('-NoProfile','-Command','Start-Sleep -Seconds 30') -NoNewWindow -PassThru; Wait-Process -Id $p.Id"#
        ],
        "timeoutMs": 50,
    });
    #[cfg(not(windows))]
    let opts = serde_json::json!({
        "bin": "sh",
        "args": ["-c", "sleep 2"],
        "timeoutMs": 50,
    });

    // Windows: budget for slow PowerShell interpreter startup under CI
    // contention (measured well before the 30s a real regression would take).
    #[cfg(windows)]
    let max_elapsed = Duration::from_secs(8);
    #[cfg(not(windows))]
    let max_elapsed = Duration::from_secs(1);

    let started = Instant::now();
    let out: serde_json::Value = serde_json::from_str(&run(&opts.to_string())).unwrap();
    assert!(
        started.elapsed() < max_elapsed,
        "timeout waited for descendant pipe handles to close: {:?}",
        started.elapsed()
    );
    assert!(out
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .contains("timed out"));
}
