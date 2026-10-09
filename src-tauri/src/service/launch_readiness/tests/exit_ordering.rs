fn lifecycle_identity_clear_source() -> &'static str {
    include_str!("../../tab/lifecycle_exit.rs")
        .split("fn clear_tab_agent_identity_for_lifecycle_exit_during(")
        .nth(1)
        .unwrap()
        .split("fn lifecycle_clear_has_inflight_launch(")
        .next()
        .unwrap()
}

/// The second half of the same live failure: every observer that sees an exit
/// also tears the tab's agent identity down, and the teardown clears the launch
/// nonce. A settlement attempted afterwards can only be Superseded, so the
/// settle must come first at every call site.
#[test]
fn launch_readiness_exit_settles_before_identity_teardown() {
    let tab = launching_tab("pty-teardown", "nonce-teardown");
    let attempt = attempt_of(&tab);
    let proof = exit_proof(&attempt);

    assert!(matches!(
        settle_proven_exit(&tab, &attempt, Some(3), &proof),
        LaunchSettlement::Failed { .. }
    ));

    let mut torn_down = tab.clone();
    assert!(crate::service::tab::clear_agent_identity_for_lifecycle_exit(&mut torn_down));
    assert!(torn_down.session_binding_nonce.is_none());
    assert_eq!(
        settle_proven_exit(&torn_down, &attempt, Some(3), &proof),
        LaunchSettlement::Superseded,
    );

    // Claim, tear down, then publish: the verdict has to be decided while the
    // tab still carries the launch, and recorded after the teardown resets it.
    for (source, teardown_anchor) in [
        (
            include_str!("../../output/shell_markers.rs"),
            "clear_tab_agent_identity_after_exit_with_proof",
        ),
        (
            lifecycle_identity_clear_source(),
            "let finished = self.finish_agent_identity_clear",
        ),
    ] {
        let claim = source
            .find("claim_launch_exit")
            .expect("the exit source claims the launch");
        let teardown = source
            .find(teardown_anchor)
            .expect("the exit source tears identity down");
        let publish = source
            .find("publish_launch_exit")
            .expect("the exit source publishes the verdict");
        let handoff = source
            .find("handoff_launch_exit_after_teardown")
            .expect("recovery starts at the explicit post-teardown handoff");
        assert!(claim < teardown, "the claim must precede the teardown");
        assert!(teardown < publish, "the verdict must be published after it");
        assert!(
            publish < handoff,
            "recovery must follow verdict publication"
        );
    }
}

/// A launch verdict is published after the teardown, so it is the stage that
/// stands even though the teardown resets the tab to `shell_ready`.
#[test]
fn launch_readiness_failed_launch_is_published_after_identity_teardown() {
    let mut torn_down = launching_tab("pty-verdict", "nonce-verdict");
    assert!(crate::service::tab::clear_agent_identity_for_lifecycle_exit(&mut torn_down));
    assert_eq!(torn_down.boot_stage, Some(BootStage::ShellReady));

    let tab_source = lifecycle_identity_clear_source();
    let clear = tab_source
        .find("let finished = self.finish_agent_identity_clear")
        .expect("the funnel clears identity");
    let publish = tab_source
        .find("publish_launch_exit")
        .expect("the funnel publishes the claimed verdict");
    assert!(
        clear < publish,
        "the verdict must be published after the clear"
    );
    let handoff = tab_source
        .find("handoff_launch_exit_after_teardown")
        .expect("recovery starts at the explicit post-teardown handoff");
    assert!(publish < handoff, "recovery follows teardown publication");

    let output = include_str!("../../output/shell_markers.rs");
    let teardown = output
        .find("clear_tab_agent_identity_after_exit_with_proof")
        .expect("the shell exit source tears identity down");
    let published = output
        .find("publish_launch_exit")
        .expect("the shell exit source publishes its verdict");
    assert!(teardown < published, "publish must follow the teardown");
}
