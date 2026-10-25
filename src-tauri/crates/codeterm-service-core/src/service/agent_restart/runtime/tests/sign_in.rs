use super::*;

const SIGNED_OUT_SOURCE: &str = r#"
module.exports = {
  runtimePlan(ctx, op) {
    return {action:{kind:'request',call:{method:op.kind,params:op}},state:null};
  },
  runtimeResponse(ctx, op) {
    return {kind:'complete',result:{kind:op.kind === 'initialize' ? 'initialized' : 'done'},events:[{kind:'auth',state:'signedOut',detail:'opaque-auth-detail'}]};
  },
  runtimeReduce() { return {state:null,events:[]}; }
};
"#;

#[test]
fn signed_out_restart_is_accepted_pending_without_latching_launch_failure() {
    for fresh in [false, true] {
        let fixture = RuntimeFixture::with_source(SIGNED_OUT_SOURCE);
        let outcome = fixture
            .service
            .agent_restart_observed_with_options(
                &fixture.tab,
                RestartOptions {
                    fresh,
                    force: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(outcome.restart_accepted);
        assert!(!outcome.agent_ready);
        assert!(!outcome.binding_restored);
        let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
        assert_eq!(session.state.lock().snapshot().state, "loginPending");
        assert!(session.state.lock().snapshot().reason.is_none());
        assert_ne!(
            fixture
                .service
                .mux
                .read()
                .get_tab(&fixture.tab)
                .unwrap()
                .boot_stage,
            Some(codeterm_types::pty::BootStage::SpawnFailed)
        );
        assert!(!fixture
            .requests()
            .iter()
            .any(|request| request["method"] == "submitTurn"));
    }
}
