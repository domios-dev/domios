use super::*;
use crate::provider::{AgentLaunchParams, AgentResumeParams, LaunchSpecParams};

#[test]
fn scoped_permission_matrix_is_identical_in_native_and_plugin_fresh_resume_restore_specs() {
    if require_plugins().is_none() {
        return;
    }
    let native = crate::agent_claude::provider::ClaudeProvider::new();
    let js = load_js("claude");
    let root = tempfile::tempdir().unwrap();
    codeterm_util::task_files::capture_in(root.path(), "scoped-parity", || {
        for access in ["read_only", "folder", "full"] {
            for lifecycle in ["fresh", "resume", "restore"] {
                let full = access == "full";
                let args = if full {
                    Vec::new()
                } else {
                    let cap = native.launch_permission_capability().unwrap();
                    cap.scoped_filesystem_args(
                        access == "read_only",
                        &if access == "folder" {
                            vec!["/opaque friend".into(), "/opaque pad".into()]
                        } else {
                            vec![]
                        },
                        &if access == "folder" {
                            vec!["/opaque friend".into(), "/opaque pad".into()]
                        } else {
                            vec!["/opaque pad".into()]
                        },
                    )
                    .unwrap()
                };
                let fresh = AgentLaunchParams {
                    args: Some(args.clone()),
                    skip_permissions: full,
                    task: Some("opaque scope".into()),
                    ..Default::default()
                };
                let resume = AgentResumeParams {
                    session_id: "opaque-session".into(),
                    args: args.clone(),
                    skip_permissions: full,
                    system_prompt: Some("opaque scope".into()),
                    ..Default::default()
                };
                let input = if lifecycle == "fresh" {
                    LaunchSpecParams::Fresh(&fresh)
                } else {
                    LaunchSpecParams::Resume(&resume)
                };
                let actual =
                    launch_spec::materialized_arguments(native.launch_spec(&input).unwrap());
                let plugin = launch_spec::materialized_arguments(js.launch_spec(&input).unwrap());
                assert_eq!(actual, plugin);
                for argument in &args {
                    assert!(actual.args.contains(argument));
                }
                assert_eq!(
                    actual
                        .args
                        .iter()
                        .any(|arg| arg == "--dangerously-skip-permissions"),
                    full
                );
                assert_eq!(
                    actual
                        .args
                        .windows(2)
                        .any(|pair| pair == ["--permission-mode", "dontAsk"]),
                    !full
                );
            }
        }
    });
}
