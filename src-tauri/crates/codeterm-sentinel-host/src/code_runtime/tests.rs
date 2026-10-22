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
    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("outside-secret");
    let forbidden = outside.path().join("forbidden-write");
    std::fs::write(&secret, "opaque-denied-content").unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let read = serde_json::to_string(&secret).unwrap();
    let write = serde_json::to_string(&forbidden).unwrap();
    let input = br#"{"media":"json","schema_id":null,"value":{"n":7},"text":""}"#;
    let python = format!(
        r#"import json,sys,os,socket
x=json.load(sys.stdin)
assert x['value']['n']==7
assert not any(k in os.environ for k in ['OPENAI_API_KEY','CODETERM_API_TOKEN','BLUEPRINT_FAKE_API_KEY','DOMIOS_FAKE_CONFINEMENT_TOKEN'])
for path,mode in [({read},'r'),({write},'w')]:
 try:
  open(path,mode)
  raise AssertionError('ambient filesystem allowed')
 except PermissionError: pass
try:
 s=socket.socket()
 s.connect(('127.0.0.1',{port}))
 raise AssertionError('network allowed')
except PermissionError: pass
x['value']['n']+=1
json.dump(x,sys.stdout)
"#,
        port = address.port()
    );
    let ts = format!(
        r#"import fs from 'node:fs';
import net from 'node:net';
const x: {{value:{{n:number}}}} = JSON.parse(fs.readFileSync(0,'utf8'));
if (process.env.OPENAI_API_KEY || process.env.CODETERM_API_TOKEN || process.env.BLUEPRINT_FAKE_API_KEY || process.env.DOMIOS_FAKE_CONFINEMENT_TOKEN) throw Error('credential inherited');
for (const [path,mode] of [[{read},'r'],[{write},'w']]) {{
 try {{ fs.openSync(path,mode); throw Error('ambient filesystem allowed'); }}
 catch (e) {{ if (!['EPERM','EACCES'].includes(e.code)) throw e; }}
}}
try {{ eval('1+1'); throw Error('string code generation allowed'); }} catch(e) {{ if (!(e instanceof EvalError)) throw e; }}
const socket=net.connect({port},'127.0.0.1');
socket.on('connect',()=>{{throw Error('network allowed')}});
socket.on('error',(e)=>{{ if(!['EPERM','EACCES'].includes(e.code)) throw e; x.value.n++; process.stdout.write(JSON.stringify(x)); }});
"#,
        port = address.port()
    );
    let owner = Arc::new(ProbeResourceOwner::new());
    let mut failures = Vec::new();
    for (lang, src) in [(Language::Python, python), (Language::Ts, ts)] {
        let code = Code {
            lang,
            src: Some(src),
            file: None,
            caps: vec![],
            limits: Limits::default(),
        };
        let result = run(
            &code,
            input,
            Duration::from_secs(3),
            &owner,
            &Cancellation::default(),
        )
        .expect("native sandbox must execute; unavailable confinement is a FAILED proof");
        if !result.success { failures.push(format!("native {:?} failed: {}",code.lang,result.text)); continue; }
        let value: serde_json::Value = serde_json::from_str(&result.stdout_text()).unwrap();
        assert_eq!(value["value"]["n"], 8);
        assert!(!forbidden.exists());
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(owner.snapshot().unconfirmed, 0);
}
