use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;

#[test]
fn shell_output_settles_wake_only_after_own_prompt_evidence() {
    for prompt in [
        b"\x1b]133;A\x07".as_slice(),
        b"PS C:\\project> ",
        b"C:\\project>",
    ] {
        let (mut mux, id) = MultiplexerState::new();
        mux.link_pty(&id, "shell-pty".into());
        mux.get_tab_mut(&id).unwrap().boot_stage = Some(BootStage::PtyStarting);
        let fixture = ServiceFixture::new(mux);
        fixture
            .service
            .screen_state
            .bind_pty_generation("shell-pty", 7);
        fixture
            .service
            .feed_pty_output_for_generation("shell-pty", 7, b"Starting shell\r\n");
        assert_eq!(
            fixture.service.mux.read().get_tab(&id).unwrap().boot_stage,
            Some(BootStage::PtyStarting)
        );
        fixture
            .service
            .feed_pty_output_for_generation("shell-pty", 7, prompt);
        assert_eq!(
            fixture.service.mux.read().get_tab(&id).unwrap().boot_stage,
            Some(BootStage::ShellReady)
        );
    }
}

#[test]
fn stale_output_and_agent_shell_prompts_cannot_settle_a_launch() {
    for (agent, generation) in [(false, 6), (true, 7)] {
        let (mut mux, id) = MultiplexerState::new();
        mux.link_pty(&id, "shell-pty".into());
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.boot_stage = Some(BootStage::PtyStarting);
        tab.agent_type = agent.then(|| "fixture".into());
        let fixture = ServiceFixture::new(mux);
        fixture
            .service
            .screen_state
            .bind_pty_generation("shell-pty", 7);
        fixture
            .service
            .feed_pty_output_for_generation("shell-pty", generation, b"\x1b]133;A\x07");
        assert_eq!(
            fixture.service.mux.read().get_tab(&id).unwrap().boot_stage,
            Some(BootStage::PtyStarting)
        );
    }
}

#[test]
fn prompt_observed_before_link_is_reconciled_for_the_selected_shell() {
    let (mut mux, id) = MultiplexerState::new();
    mux.get_tab_mut(&id).unwrap().boot_stage = Some(BootStage::PtyStarting);
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .screen_state
        .bind_pty_generation("shell-pty", 7);
    fixture
        .service
        .feed_pty_output_for_generation("shell-pty", 7, b"\x1b]133;A\x07");
    fixture
        .service
        .mux
        .write()
        .link_pty(&id, "shell-pty".into());
    fixture.service.observe_shell_ready("shell-pty", Some(7));
    assert_eq!(
        fixture.service.mux.read().get_tab(&id).unwrap().boot_stage,
        Some(BootStage::ShellReady)
    );
}
