use super::*;
use codeterm_decision::physical::{self, RequestScope};
use std::time::{Duration, Instant};

#[test]
fn durable_media_observation_denies_real_effect_admission_and_preserves_reads() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("retained.txt");
    std::fs::write(&path, "retained").unwrap();
    let missing = temp.path().join("never-created");
    let vm = vm_with(
        r#"module.exports={
        attack(p,d){ return {
            read:host.readFile(p),write:host.writeFile(p,'changed'),atomic:host.writeFileAtomic(p,'changed'),
            remove:host.removeFile(p),mkdir:host.makeDirs(d),temp:host.writeTempFile('changed','.txt'),lock:host.lockFile(p),
            exec:JSON.parse(host.exec(JSON.stringify({bin:'never-execute-fixture',args:[]}))),queuedExec:host.exec.start({bin:'never-execute-fixture',args:[]}),
            fetch:JSON.parse(host.fetch(JSON.stringify({url:'https://example.invalid/'}))),stream:host.fetch.stream({url:'https://example.invalid/'}),
            secret:host.secretSet('fixture','changed'),worker:host.worker.start({}),agent:host.agent.spawn('workspace',{}),
            capture:host.localRecording.setEnabled(true)
        }; },
        queued(){return host.fetch.async({url:'https://example.invalid/'},function(result){return {result:result,write:host.writeTempFile('changed','.txt')};});},
        write(p){return host.writeFile(p,'normal');}
    };"#,
    );
    let denied = {
        let _scope = RequestScope::observation_only(Instant::now() + Duration::from_secs(2));
        let denied: serde_json::Value =
            crate::js_runtime::vm::call_json(&vm, "attack", &[json!(path), json!(missing)])
                .unwrap();
        let queued: serde_json::Value =
            crate::js_runtime::vm::call_json(&vm, "queued", &[]).unwrap();
        assert!(queued["result"]["error"]
            .as_str()
            .unwrap()
            .contains("denied"));
        assert!(queued["write"].is_null());
        denied
    };
    assert_eq!(denied["read"], "retained");
    for field in ["write", "atomic", "remove", "mkdir", "secret"] {
        assert_eq!(denied[field], false, "{field}");
    }
    for field in ["temp", "lock"] {
        assert!(denied[field].is_null(), "{field}");
    }
    for field in ["exec", "queuedExec", "fetch", "stream"] {
        assert!(
            denied[field]["error"].as_str().unwrap().contains("denied"),
            "{field}"
        );
    }
    for field in ["worker", "agent", "capture"] {
        assert!(denied[field]["error"].is_string(), "{field}");
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "retained");
    assert!(!missing.exists());
    assert!(physical::effects_allowed());
    assert!(crate::js_runtime::vm::call_json::<bool>(&vm, "write", &[json!(path)]).unwrap());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "normal");
}

#[test]
fn durable_media_observation_refuses_cached_grants_and_populated_service_before_effects() {
    let temp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(temp.path()).unwrap();
    let sandbox = PluginSandbox::unrestricted()
        .with_identity_and_perms(
            "lookup-grants",
            &crate::js_runtime::manifest::PermissionsConfig {
                local_recording: true,
                ..Default::default()
            },
        )
        .with_project_roots(
            vec![root.clone()],
            vec![root.clone()],
            root.join("grants.json"),
        );
    let host: Arc<dyn PluginHostCtx> =
        Arc::new(crate::js_runtime::host_compute::tests::AnalyticsHost::default());
    sandbox.set_service(Arc::downgrade(&host));
    let path = root.join("file.txt").to_string_lossy().into_owned();
    let root = root.to_string_lossy().into_owned();
    assert!(sandbox.can_exec("fixture"));
    assert!(sandbox.can_reach("example.invalid"));
    assert!(sandbox.can_write(&path));
    assert!(sandbox.project_write_allowed(&root));
    assert!(sandbox.service().is_some());
    assert!(sandbox.local_recording_allowed());
    assert!(sandbox.lifecycle_permission_allowed("spawn_agents"));
    {
        let _scope = RequestScope::observation_only(Instant::now() + Duration::from_secs(2));
        assert!(!sandbox.can_exec("fixture"));
        assert!(!sandbox.can_reach("example.invalid"));
        assert!(!sandbox.can_write(&path));
        assert!(!sandbox.project_write_allowed(&root));
        assert!(sandbox.service().is_none());
        assert!(!sandbox.local_recording_allowed());
        assert!(!sandbox.lifecycle_permission_allowed("spawn_agents"));
    }
    assert!(sandbox.service().is_some());
    assert!(sandbox.can_exec("fixture"));
}

#[test]
fn durable_media_observation_cannot_consume_or_close_an_admitted_effect_receipt() {
    use crate::js_runtime::{host_exec, vm::call_json};
    use std::sync::mpsc;
    let (started, entered) = mpsc::channel();
    let (release, held) = mpsc::channel();
    let job = host_exec::start_json_job("lookup-effects-receipt", move || {
        started.send(()).unwrap();
        held.recv().unwrap();
        json!({"job":"already-admitted","state":"queued"}).to_string()
    })
    .unwrap();
    entered.recv_timeout(Duration::from_secs(2)).unwrap();
    let vm = PluginVm::from_source(
        "lookup-effects-receipt",
        r#"module.exports={
        close(job){host.exec.close(job);return true;},
        poll(job){return host.exec.poll(job);},
        await(job){return host.awaitJob(job,function(receipt){return receipt;});}
    };"#
        .into(),
    )
    .unwrap();
    {
        let _scope = RequestScope::observation_only(Instant::now() + Duration::from_secs(2));
        assert!(call_json::<bool>(&vm, "close", &[json!(job)]).unwrap());
        let refused: serde_json::Value = call_json(&vm, "poll", &[json!(job)]).unwrap();
        assert_eq!(
            refused["error"],
            "effect job unavailable during observation"
        );
        let refused = call_json::<serde_json::Value>(&vm, "await", &[json!(job)]).unwrap_err();
        assert!(
            matches!(refused, crate::js_runtime::vm::VmError::Script(ref message)
            if message == "await job is foreign, unavailable or denied")
        );
    }
    let pending: serde_json::Value =
        serde_json::from_str(&host_exec::poll("lookup-effects-receipt", &job)).unwrap();
    assert_eq!(pending["done"], false);
    assert!(pending.get("error").is_none());
    release.send(()).unwrap();
    let host_exec::WaitResult::Completed(receipt) =
        host_exec::wait_json("lookup-effects-receipt", &job)
    else {
        panic!("fixture receipt did not settle");
    };
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&receipt).unwrap()["job"],
        "already-admitted"
    );
}

#[test]
fn durable_media_observation_allows_real_read_query_continuation_without_restoring_effects() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("lookup.sqlite");
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute_batch("CREATE TABLE source(value TEXT); INSERT INTO source VALUES('retained');")
        .unwrap();
    let output = temp.path().join("must-not-exist.txt");
    let vm = PluginVm::from_source("lookup-read-continuation", r#"module.exports={
        query(db,out){return host.fs.sqliteQueryAsync(db,'SELECT value FROM source',[],function(rows){return {rows:rows,written:host.writeFile(out,'changed')};});}
    };"#.into()).unwrap();
    let result: serde_json::Value = {
        let deadline = Instant::now() + Duration::from_secs(2);
        let _scope = RequestScope::observation_only(deadline);
        crate::js_runtime::vm::call_json_interruptible(
            &vm,
            "query",
            &[json!(database), json!(output)],
            Default::default(),
            deadline,
        )
        .unwrap()
    };
    assert_eq!(result["rows"][0]["value"], "retained");
    assert_eq!(result["written"], false);
    assert!(!output.exists());
}
