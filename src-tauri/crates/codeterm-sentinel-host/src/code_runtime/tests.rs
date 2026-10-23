use super::*;
use codeterm_blueprint_types::blueprint_document::Limits;

fn body(lang: Language, src: &str) -> Code {
    Code {
        lang,
        src: Some(src.into()),
        file: None,
        caps: vec![],
        limits: Limits::default(),
    }
}

const INPUT: &[u8] = br#"{"media":"json","schema_id":null,"value":{"n":7},"text":""}"#;

#[test]
fn blueprint_v2_runtime_native_python_and_ts_children() {
    let owner = Arc::new(ProbeResourceOwner::new());
    for (lang, src) in [
        (Language::Python, "import os\ndef main(value):\n assert not any(k.upper().startswith(('CODETERM', 'DOMIOS')) for k in os.environ)\n assert not any(k in os.environ for k in ['OPENAI_API_KEY', 'BLUEPRINT_FAKE_API_KEY'])\n return {'n': value['n'] + 1}\n"),
        (Language::Ts, "export default function main(value: {n:number}) { if (Object.keys(process.env).some(k => /^(CODETERM|DOMIOS)/i.test(k)) || process.env.OPENAI_API_KEY || process.env.BLUEPRINT_FAKE_API_KEY) throw Error('credential inherited'); return {n: value.n + 1}; }")
    ] {
        let output = run(&body(lang, src), INPUT, Duration::from_secs(3), &owner, &Cancellation::default()).unwrap();
        assert!(output.success, "{}", output.text);
        let value: serde_json::Value = serde_json::from_str(&output.stdout_text()).unwrap();
        assert_eq!(value["value"], serde_json::json!({"n":8}));
    }
    assert_eq!(owner.snapshot().unconfirmed, 0);
}

#[test]
fn blueprint_v2_runtime_code_children_keep_user_file_permissions() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let target = root.path().join("target");
    std::fs::write(&source, "opaque").unwrap();
    let owner = Arc::new(ProbeResourceOwner::new());
    let input = serde_json::to_vec(&serde_json::json!({"media":"json","schema_id":null,"value":{"source":source,"target":target},"text":""})).unwrap();
    for (lang, src) in [
        (Language::Python, "def main(value):\n with open(value['source']) as src: text=src.read()\n with open(value['target'],'w') as dest: dest.write(text)\n return text\n"),
        (Language::Ts, "import fs from 'node:fs';\nexport default function main(value) { const text = fs.readFileSync(value.source,'utf8'); fs.writeFileSync(value.target,text); return text; }")
    ] {
        let output = run(&body(lang, src), &input, Duration::from_secs(3), &owner, &Cancellation::default()).unwrap();
        assert!(output.success, "{}", output.text);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "opaque");
        assert_eq!(serde_json::from_str::<serde_json::Value>(&output.stdout_text()).unwrap()["value"], "opaque");
        std::fs::remove_file(&target).unwrap();
    }
}

#[test]
fn blueprint_v2_runtime_native_deadline_and_output_budgets() {
    let owner = Arc::new(ProbeResourceOwner::new());
    for (src, limits, timeout, reason) in [
        (
            "def main(value):\n while True: pass\n",
            Limits::default(),
            Duration::from_millis(300),
            "deadline",
        ),
        (
            "def main(value):\n print('x'*65536)\n",
            Limits {
                output_bytes: Some(256),
                ..Default::default()
            },
            Duration::from_secs(3),
            "budget_exhausted",
        ),
    ] {
        let mut code = body(Language::Python, src);
        code.limits = limits;
        let failure = run(&code, INPUT, timeout, &owner, &Cancellation::default()).unwrap_err();
        assert_eq!(failure.reason, reason);
        assert!(!failure.uncertain, "{failure:?}");
        assert_eq!(owner.snapshot().unconfirmed, 0);
    }
}

#[cfg(any(target_os = "linux", windows))]
fn prove_memory_limit() {
    let owner = Arc::new(ProbeResourceOwner::new());
    let src = "def main(value):\n try:\n  data = bytearray(1024 * 1024 * 1024)\n except MemoryError:\n  return {'limited': True}\n return {'limited': False}\n";
    let mut code = body(Language::Python, src);
    code.limits.mem_mb = Some(128);
    let output = run(
        &code,
        INPUT,
        Duration::from_secs(3),
        &owner,
        &Cancellation::default(),
    )
    .unwrap();
    assert!(output.success, "{}", output.text);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&output.stdout_text()).unwrap()["value"],
        serde_json::json!({"limited":true})
    );
    assert_eq!(owner.snapshot().unconfirmed, 0);
}

#[cfg(target_os = "linux")]
#[test]
fn blueprint_v2_runtime_linux_memory_limit_is_enforced() {
    prove_memory_limit();
}

#[cfg(windows)]
#[test]
fn blueprint_v2_runtime_windows_job_memory_limit_is_enforced() {
    prove_memory_limit();
}

#[test]
fn blueprint_v2_runtime_native_cancellation_uses_child_events() {
    let cancellation = Cancellation::default();
    let waits = cancellation.observe_waits();
    let owner = Arc::new(ProbeResourceOwner::new());
    let code = body(Language::Python, "def main(value):\n while True: pass\n");
    std::thread::scope(|scope| {
        let child =
            scope.spawn(|| run(&code, INPUT, Duration::from_secs(3), &owner, &cancellation));
        waits.recv_timeout(Duration::from_secs(2)).unwrap();
        cancellation.cancel();
        let failure = child.join().unwrap().unwrap_err();
        assert_eq!(failure.reason, "skip:lease_cancelled");
        assert!(!failure.uncertain);
    });
    assert_eq!(owner.snapshot().unconfirmed, 0);
}

#[cfg(unix)]
#[test]
fn blueprint_v2_runtime_code_file_refuses_symlink_escape() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(
        outside.path().join("secret.py"),
        "raise Exception('outside')",
    )
    .unwrap();
    symlink(outside.path(), root.path().join("escape")).unwrap();
    assert_eq!(
        codeterm_sentinel::code_source::read(root.path(), "escape/secret.py").unwrap_err(),
        "contract_violation"
    );
    std::fs::write(
        root.path().join("fixed.py"),
        "def main(value): return value",
    )
    .unwrap();
    assert_eq!(
        codeterm_sentinel::code_source::read(root.path(), "fixed.py").unwrap(),
        "def main(value): return value"
    );
}

#[test]
fn blueprint_v2_runtime_child_environment_removes_credentials_and_cli_path() {
    use crate::probe::environment::Environment;
    use std::ffi::OsString;
    let root = tempfile::tempdir().unwrap();
    let cli = root.path().join("private-cli");
    std::fs::create_dir(&cli).unwrap();
    let path = std::env::var_os("PATH").unwrap();
    let path =
        std::env::join_paths(std::iter::once(cli.clone()).chain(std::env::split_paths(&path)))
            .unwrap();
    let inherited = std::env::vars_os()
        .filter(|(name, _)| !name.eq_ignore_ascii_case("PATH"))
        .chain([
            (OsString::from("PATH"), path),
            (
                "CODETERM_CLI_PATH".into(),
                cli.join("domios").into_os_string(),
            ),
            ("CODETERM_API_TOKEN".into(), "opaque-instance-secret".into()),
            ("DOMIOS_FAKE_TOKEN".into(), "opaque-second-secret".into()),
            ("OPENAI_API_KEY".into(), "opaque-provider-secret".into()),
            (
                "BLUEPRINT_FAKE_API_KEY".into(),
                "opaque-third-secret".into(),
            ),
        ]);
    let environment = Environment::for_command(inherited, None, &[]);
    let owner = Arc::new(ProbeResourceOwner::new());
    for (lang, src) in [
        (Language::Python, "import os\ndef main(value):\n assert not any(k.upper().startswith(('CODETERM','DOMIOS')) for k in os.environ)\n assert not any(k in os.environ for k in ['OPENAI_API_KEY','BLUEPRINT_FAKE_API_KEY'])\n return os.environ['PATH']\n"),
        (Language::Ts, "export default function main(value) { if (Object.keys(process.env).some(k => /^(CODETERM|DOMIOS)/i.test(k)) || process.env.OPENAI_API_KEY || process.env.BLUEPRINT_FAKE_API_KEY) throw Error('credential inherited'); return process.env.PATH; }")
    ] {
        let output = child::execute(&body(lang, src), INPUT, Duration::from_secs(60), Duration::from_secs(3), &owner, &Cancellation::default(), None, &environment).unwrap();
        assert!(output.success, "{}", output.text);
        let value = serde_json::from_str::<serde_json::Value>(&output.stdout_text()).unwrap();
        let child_path = value["value"].as_str().unwrap();
        assert!(!std::env::split_paths(child_path).any(|directory| directory == cli));
        for secret in ["opaque-instance-secret", "opaque-second-secret", "opaque-provider-secret", "opaque-third-secret"] {
            assert!(!output.text.contains(secret));
        }
    }
}
