#[test]
fn event_paths_have_no_plan_reminder_injection() {
    let cases = [
        (
            "report completion",
            include_str!("../../../codeterm-service-core/src/service/hierarchy.rs"),
            concat!("maybe_send_", "plan_reminder("),
        ),
        (
            "HTTP owned-tab send",
            include_str!("../tab_routes/send.rs"),
            concat!("maybe_send_owned_", "plan_reminder("),
        ),
        (
            "socket owned-tab send",
            include_str!("../../../codeterm-service-core/src/socket/server/tab.rs"),
            concat!("maybe_send_owned_", "plan_reminder("),
        ),
        (
            "HTTP text polling",
            include_str!("../tab_routes.rs"),
            concat!("maybe_send_polling_", "nudge("),
        ),
        (
            "HTTP chat polling",
            include_str!("../tab_chat_routes.rs"),
            concat!("maybe_send_polling_", "nudge("),
        ),
        (
            "socket output polling",
            include_str!("../../../codeterm-service-core/src/socket/server/tab.rs"),
            concat!("maybe_send_polling_", "nudge("),
        ),
        (
            "idle transition",
            include_str!("../../../codeterm-service-core/src/startup/background.rs"),
            concat!("maybe_send_idle_", "plan_poke("),
        ),
    ];

    for (event, source, obsolete_call) in cases {
        assert!(
            !source.contains(obsolete_call),
            "{event} must not inject a periodic plan reminder"
        );
    }

    let hierarchy = include_str!("../../../codeterm-service-core/src/service/hierarchy.rs");
    assert!(hierarchy.contains("report.inject_body()"));
    assert!(hierarchy.contains("pub fn notify_owner_task_in_review"));
    assert!(hierarchy.contains("pub fn nudge_gate_asker"));
}
