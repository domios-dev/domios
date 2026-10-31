use super::*;
use crate::host_traits::PluginHostCtx;
use crate::{
    agent_commands::AgentCommandProvider,
    js_runtime::{
        host_api::PluginSandbox,
        host_compute::tests::AnalyticsHost,
        manifest::PermissionsConfig,
        vm::{call_json, PluginVm},
        JsAgentCommandHandler,
    },
};
use serde_json::{json, Value};

fn fixture(source: String, root: &std::path::Path) -> (Arc<PluginVm>, Arc<dyn PluginHostCtx>) {
    let sandbox = Arc::new(
        PluginSandbox::restricted(vec![root.to_path_buf()], vec![]).with_identity_and_perms(
            "origin-fixture",
            &PermissionsConfig {
                surfaces: true,
                agent_commands: true,
                ..Default::default()
            },
        ),
    );
    let vm =
        Arc::new(PluginVm::from_source_sandboxed("origin-fixture", source, None, sandbox).unwrap());
    let host: Arc<dyn PluginHostCtx> = Arc::new(AnalyticsHost::default());
    assert!(vm.set_service(Arc::downgrade(&host)));
    (vm, host)
}

#[test]
fn surface_job_origin_survives_real_sqlite_continuation_and_is_absent_after_final_return_or_raw_plan(
) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("read.sqlite");
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE sample(value INTEGER); INSERT INTO sample VALUES(42);")
        .unwrap();
    let source = r#"module.exports.default={
        onAgentCommand(c){if(c.verb==='raw'){host.surfaceCall('origin-sync','[]');return {result:JSON.stringify(host.receiverTarget.get())};}
            if(c.verb==='origin-sync'){var result=JSON.parse(host.surface.call(c.verb,'[]'));return {result:JSON.stringify({inside:JSON.parse(result.result),after:host.receiverTarget.get()})};}
            return host.surface.dispatch(c.verb,[]);},
        onSurfaceCall(c,verb){var first=host.receiverTarget.get();if(verb==='origin-sync')return JSON.stringify(first);
            return host.fs.sqliteQueryAsync(DB,'SELECT value FROM sample',[],function(rows){return JSON.stringify({first:first,inside:host.receiverTarget.get(),rows:rows});});},
        outside(){return host.receiverTarget.get();}
    };"#.replace("DB", &json!(path.to_string_lossy()).to_string());
    let (vm, _host) = fixture(source, root.path());
    let handler = JsAgentCommandHandler::new(vm.clone(), "origin-fixture", true);
    let out = handler
        .on_agent_command("authenticated-agent", "origin-probe", &[], None)
        .unwrap();
    let out: Value = serde_json::from_str(&out.result).unwrap();
    assert_eq!(out["rows"][0]["value"], 42);
    assert_eq!(out["first"]["origin"], out["inside"]["origin"]);
    assert_eq!(out["inside"]["origin"]["actor"], "authenticated-agent");
    assert_eq!(out["inside"]["origin"]["owner"], "owner");
    let sync = handler
        .on_agent_command("authenticated-agent", "origin-sync", &[], None)
        .unwrap();
    let sync: Value = serde_json::from_str(&sync.result).unwrap();
    assert_eq!(sync["inside"]["origin"]["actor"], "authenticated-agent");
    assert_eq!(sync["after"]["error"], "surface_origin_unavailable");
    let raw = handler
        .on_agent_command("authenticated-agent", "raw", &[], None)
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&raw.result).unwrap()["error"],
        "surface_origin_unavailable"
    );
    assert_eq!(
        call_json::<Value>(&vm, "outside", &[]).unwrap()["error"],
        "surface_origin_unavailable"
    );
    assert!(crate::surface_invocation::current("origin-fixture").is_none());
}

#[test]
fn surface_continuations_refuse_foreign_jobs_and_stale_root_markers_without_consuming_receipts() {
    let root = tempfile::tempdir().unwrap();
    let source = r#"module.exports.default={saved:null,
        seed(job){this.saved=host.awaitJob(job,function(){return 'must not resume';});return 'saved';},
        replay(){return this.saved;},foreign(job){return host.awaitJob(job,function(){throw Error('foreign callback executed');});},
        steal(k){return typeof globalThis.__ct_await_take__(k);}
    };"#;
    let (vm, _host) = fixture(source.into(), root.path());
    let owned = crate::js_runtime::host_exec::start_retained_json_job("origin-fixture", || {
        json!({"value":42}).to_string()
    })
    .unwrap();
    assert_eq!(
        call_json::<String>(&vm, "seed", &[json!(owned)]).unwrap(),
        "saved"
    );
    assert!(call_json::<Value>(&vm, "replay", &[]).is_err());
    assert_eq!(
        call_json::<String>(&vm, "steal", &[json!("1")]).unwrap(),
        "undefined"
    );
    let foreign = crate::js_runtime::host_exec::start_retained_json_job("other-plugin", || {
        json!({"value":7}).to_string()
    })
    .unwrap();
    assert!(call_json::<Value>(&vm, "foreign", &[json!(foreign)]).is_err());
    for (plugin, job, expected) in [
        ("origin-fixture", &owned, 42),
        ("other-plugin", &foreign, 7),
    ] {
        let crate::js_runtime::host_exec::WaitResult::Completed(value) =
            crate::js_runtime::host_exec::wait_json(plugin, job)
        else {
            panic!("known receipt disappeared");
        };
        assert_eq!(
            serde_json::from_str::<Value>(&value).unwrap()["value"],
            expected
        );
    }
}

#[test]
fn surface_origin_is_revoked_when_cancelled_while_awaiting() {
    use crate::js_runtime::{call_gate, vm::call_json_interruptible};
    use std::time::{Duration, Instant};
    let root = tempfile::tempdir().unwrap();
    let entered = format!("origin-entered-{}", uuid::Uuid::new_v4().simple());
    let release = format!("origin-release-{}", uuid::Uuid::new_v4().simple());
    let source = r#"module.exports.default={onAgentCommand(c){return host.surface.dispatch('origin-probe',[]);},
        onSurfaceCall(){if(host.receiverTarget.get().origin.actor!=='authenticated-agent')throw Error('missing origin');
          host.testGateOpen(ENTERED);return host.awaitJob(host.testGateStart(RELEASE),function(){throw Error('cancelled continuation ran');});},
        outside(){return host.receiverTarget.get();}};"#.replace("ENTERED",&json!(entered).to_string()).replace("RELEASE",&json!(release).to_string());
    let (vm, _host) = fixture(source, root.path());
    let cancellation = crate::converter::ConversionCancellation::default();
    let stop = cancellation.clone();
    let worker = vm.clone();
    let (sender, receiver) = std::sync::mpsc::channel();
    let join = std::thread::spawn(move || {
        let _caller = AgentCallerScope::enter("authenticated-agent");
        let result = call_json_interruptible::<Value>(
            &worker,
            "onAgentCommand",
            &[json!({"verb":"origin-probe"})],
            stop,
            Instant::now() + Duration::from_secs(2),
        );
        sender
            .send((
                result.is_err(),
                crate::surface_invocation::current("origin-fixture").is_none(),
            ))
            .unwrap();
    });
    let observed = call_gate::wait_opened(&entered, Duration::from_secs(2));
    cancellation.cancel();
    let completed = receiver.recv_timeout(Duration::from_secs(2));
    call_gate::open(&release);
    join.join().unwrap();
    assert!(observed);
    assert_eq!(completed.unwrap(), (true, true));
    assert_eq!(
        call_json::<Value>(&vm, "outside", &[]).unwrap()["error"],
        "surface_origin_unavailable"
    );
    call_gate::reset(&entered);
    call_gate::reset(&release);
}

#[test]
fn surface_continuation_consumes_a_same_root_receipt_only_once() {
    let root = tempfile::tempdir().unwrap();
    let (vm,_host) = fixture(r#"module.exports.default={count:0,replay(job){var self=this,marker;marker=host.awaitJob(job,function(){self.count++;return marker;});return marker;},counted(){return this.count;}};"#.into(),root.path());
    let job = crate::js_runtime::host_exec::start_retained_json_job("origin-fixture", || {
        json!({"value":1}).to_string()
    })
    .unwrap();
    assert!(call_json::<Value>(&vm, "replay", &[json!(job)]).is_err());
    assert_eq!(call_json::<u32>(&vm, "counted", &[]).unwrap(), 1);
}
