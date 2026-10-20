use super::*;

#[test]
fn tab_restart_parses_replacement_shell_parameters() {
    let parsed = TabOnly::try_parse_from([
        "tab",
        "restart",
        "--tab",
        "tab-1",
        "--cwd",
        "D:/repo",
        "--timeout",
        "10m",
        "--cmd",
        ".\\build.ps1 dev",
    ])
    .expect("tab restart parses");
    match parsed.action {
        TabAction::Restart {
            tab,
            cwd,
            cmd,
            timeout,
        } => {
            assert_eq!(timeout.as_deref(), Some("10m"));
            assert_eq!(tab.as_deref(), Some("tab-1"));
            assert_eq!(cwd.as_deref(), Some("D:/repo"));
            assert_eq!(cmd.as_deref(), Some(".\\build.ps1 dev"));
        }
        _ => panic!("expected tab restart"),
    }
}
