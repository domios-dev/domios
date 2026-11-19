use super::*;

#[test]
fn refusal_names_busy_tabs_and_preserves_idle_tabs() {
    let tab = BusyTab {
        tab_id: "utility-1".into(),
        title: "Build".into(),
        workspace: "Project".into(),
        command: "node long-task.js".into(),
    };
    let message = refusal([tab]).unwrap();
    assert!(message.contains("Build in Project: node long-task.js"));
    assert!(message.contains(r#""tab_id":"utility-1""#));
    assert!(refusal([]).is_none());
}
