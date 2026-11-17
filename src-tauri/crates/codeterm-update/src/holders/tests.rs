use super::*;

#[test]
fn refusal_names_busy_tabs_and_preserves_idle_tabs() {
    let mut tabs = vec![HolderTab {
        process: HolderProcess {
            pid: 42,
            created: 100,
        },
        tab_id: "utility-1".into(),
        title: "Build".into(),
        workspace: "Project".into(),
        command: "node long-task.js".into(),
        busy: true,
    }];
    let message = refusal(&tabs).unwrap();
    assert!(message.contains("Build in Project: node long-task.js"));
    assert!(message.contains("domios tab close --tab utility-1"));
    tabs[0].busy = false;
    assert!(refusal(&tabs).is_none());
}
