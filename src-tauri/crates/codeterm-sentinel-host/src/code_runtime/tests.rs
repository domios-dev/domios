use super::*;
#[cfg(target_os = "macos")]
use codeterm_blueprint_types::blueprint_document::Limits;

#[test]
fn blueprint_v2_runtime_platform_selection_refuses_before_launch() {
    assert!(platform_supported("macos").is_ok());
    for platform in ["windows", "linux"] {
        assert_eq!(
            platform_supported(platform).unwrap_err().reason,
            "unsupported_platform"
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn blueprint_v2_runtime_native_python_and_ts_confinement_probe() {
    struct FakeCredentials;
    impl Drop for FakeCredentials {
        fn drop(&mut self) {
            std::env::remove_var("BLUEPRINT_FAKE_API_KEY");
            std::env::remove_var("DOMIOS_FAKE_CONFINEMENT_TOKEN");
        }
    }
    let _credentials = FakeCredentials;
    std::env::set_var("BLUEPRINT_FAKE_API_KEY", "opaque-fake-api-key");
    std::env::set_var(
        "DOMIOS_FAKE_CONFINEMENT_TOKEN",
        "opaque-fake-instance-token",
    );
    let owner = Arc::new(ProbeResourceOwner::new());
    let input = br#"{"media":"json","schema_id":null,"value":{"n":7},"text":""}"#;
    for (lang, src) in [
        (Language::Python, "import os\ndef main(value):\n assert not any(k in os.environ for k in ['OPENAI_API_KEY','CODETERM_API_TOKEN','BLUEPRINT_FAKE_API_KEY','DOMIOS_FAKE_CONFINEMENT_TOKEN'])\n return {'n': value['n'] + 1}\n"),
        (Language::Ts, "export default function main(value: {n:number}) { if (process.env.OPENAI_API_KEY || process.env.CODETERM_API_TOKEN || process.env.BLUEPRINT_FAKE_API_KEY || process.env.DOMIOS_FAKE_CONFINEMENT_TOKEN) throw Error('credential inherited'); try { eval('1+1'); throw Error('string code generation allowed'); } catch(e) { if (!(e instanceof EvalError)) throw e; } return {n: value.n + 1}; }")
    ] {
        let code = Code { lang, src: Some(src.into()), file: None, caps: vec![], limits: Limits::default() };
        let result = run(&code, input, Duration::from_secs(3), &owner, &Cancellation::default())
            .expect("native confinement and interpreter availability are required");
        assert!(result.success, "{}", result.text);
        let value: serde_json::Value = serde_json::from_str(&result.stdout_text()).unwrap();
        assert_eq!(value["value"]["n"], 8);
    }
    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("secret");
    std::fs::write(&secret, "denied").unwrap();
    let path = serde_json::to_string(&secret).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    for (lang, src) in [
        (Language::Python, format!("def main(value):\n try: open({path}).read()\n except PermissionError: pass\n return value\n")),
        (Language::Ts, format!("import fs from 'node:fs';\nexport default function main(value) {{ try {{ fs.readFileSync({path}); }} catch(e) {{ if (!['EPERM','EACCES'].includes(e.code)) throw e; }} return value; }}")),
        (Language::Python, format!("import socket\ndef main(value):\n s=socket.socket()\n try: s.connect(('127.0.0.1',{port}))\n except PermissionError: pass\n return value\n")),
        (Language::Ts, format!("import net from 'node:net';\nexport default async function main(value) {{ await new Promise((resolve,reject)=>{{const s=net.connect({port},'127.0.0.1');s.on('error',reject);s.on('connect',()=>reject(Error('network allowed')));}}); return value; }}")),
    ] {
        let code = Code { lang, src: Some(src), file: None, caps: vec![], limits: Limits::default() };
        let failure = run(&code, input, Duration::from_secs(3), &owner, &Cancellation::default()).unwrap_err();
        assert_eq!(failure.reason, "sandbox_denied");
        assert!(!failure.uncertain);
    }
    assert_eq!(owner.snapshot().unconfirmed, 0);
}

#[cfg(target_os = "macos")]
#[test]
fn blueprint_v2_runtime_native_deadline_output_and_memory_budgets() {
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
        (
            "def main(value): return value",
            Limits {
                mem_mb: Some(64),
                ..Default::default()
            },
            Duration::from_secs(3),
            "contract_violation",
        ),
    ] {
        let code = Code {
            lang: Language::Python,
            src: Some(src.into()),
            file: None,
            caps: vec![],
            limits,
        };
        let result = run(
            &code,
            br#"{"media":"json","schema_id":null,"value":null,"text":""}"#,
            timeout,
            &owner,
            &Cancellation::default(),
        );
        let observed = match result {
            Err(failure) => {
                assert!(!failure.uncertain);
                failure.reason
            }
            Ok(output) => panic!("budget was not enforced: {}", output.text),
        };
        assert_eq!(observed, reason);
        assert_eq!(owner.snapshot().unconfirmed, 0);
    }
}

#[cfg(target_os = "macos")]
#[test]
fn blueprint_v2_runtime_native_cancellation_uses_child_events() {
    let cancellation = Cancellation::default();
    let waits = cancellation.observe_waits();
    let owner = Arc::new(ProbeResourceOwner::new());
    let code = Code {
        lang: Language::Python,
        src: Some("def main(value):\n while True: pass\n".into()),
        file: None,
        caps: vec![],
        limits: Limits::default(),
    };
    std::thread::scope(|scope| {
        let child = scope.spawn(|| {
            run(
                &code,
                br#"{"media":"json","schema_id":null,"value":null,"text":""}"#,
                Duration::from_secs(3),
                &owner,
                &cancellation,
            )
        });
        waits.recv_timeout(Duration::from_secs(2)).unwrap();
        cancellation.cancel();
        let failure = child.join().unwrap().unwrap_err();
        assert_eq!(failure.reason, "skip:lease_cancelled");
        assert!(!failure.uncertain);
    });
    assert_eq!(owner.snapshot().unconfirmed, 0);
}

#[cfg(target_os = "macos")]
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
        super::source::read(root.path(), "escape/secret.py")
            .unwrap_err()
            .reason,
        "sandbox_denied"
    );
    std::fs::write(root.path().join("fixed.py"), "print('fixed')").unwrap();
    assert_eq!(
        super::source::read(root.path(), "fixed.py").unwrap(),
        "print('fixed')"
    );
}
