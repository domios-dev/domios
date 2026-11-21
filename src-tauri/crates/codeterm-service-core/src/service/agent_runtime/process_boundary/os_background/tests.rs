use super::*;
use std::path::PathBuf;

fn proc_entry(
    pid: u32,
    parent_pid: u32,
    born: u128,
    argv: &[&str],
    cwd: Option<&str>,
) -> ObservedProcess {
    ObservedProcess {
        pid,
        parent_pid,
        born,
        argv: argv.iter().map(|part| (*part).to_owned()).collect(),
        cwd: cwd.map(PathBuf::from),
    }
}

#[test]
fn selects_top_level_command_roots_and_excludes_baseline_mcp_and_console_hosts() {
    let host_mcp = HostMcpFilter {
        command: "/usr/local/bin/domios".into(),
        args: vec![
            "mcp".into(),
            "serve".into(),
            "--tab".into(),
            "tab-1".into(),
            "--binding-nonce".into(),
            "nonce-1".into(),
        ],
    };
    let baseline = vec![(101, 10)];
    let observed = vec![
        proc_entry(100, 1, 1, &["harness", "acp"], Some("/repo")),
        proc_entry(101, 100, 10, &["language-server", "--stdio"], Some("/repo")),
        proc_entry(
            102,
            101,
            20,
            &[
                "/usr/local/bin/domios",
                "mcp",
                "serve",
                "--tab",
                "tab-1",
                "--binding-nonce",
                "nonce-1",
            ],
            Some("/repo"),
        ),
        proc_entry(
            103,
            101,
            30,
            &["npx", "@modelcontextprotocol/server-filesystem", "/repo"],
            Some("/repo"),
        ),
        proc_entry(104, 103, 31, &["node", "server.js"], Some("/repo")),
        proc_entry(
            200,
            101,
            50,
            &[
                "pwsh.exe",
                "-NoProfile",
                "-Command",
                "cargo nextest run -p codeterm-service-core",
            ],
            Some("/repo"),
        ),
        proc_entry(201, 200, 51, &["conhost.exe", "0x4"], Some("/repo")),
        proc_entry(
            202,
            200,
            52,
            &["cargo.exe", "nextest", "run", "-p", "codeterm-service-core"],
            Some("/repo"),
        ),
        proc_entry(203, 202, 53, &["rustc.exe", "--crate-name", "foo"], Some("/repo")),
        proc_entry(
            300,
            101,
            60,
            &["python3", "scripts/build.py", "--watch"],
            None,
        ),
    ];

    let selected = select_background_processes(100, &observed, &baseline, Some(&host_mcp), "/repo");
    assert_eq!(
        selected,
        vec![
            SelectedBackgroundProcess {
                pid: 200,
                born: 50,
                command: "cargo nextest run -p codeterm-service-core".into(),
                cwd: "/repo".into(),
            },
            SelectedBackgroundProcess {
                pid: 300,
                born: 60,
                command: "python3 scripts/build.py --watch".into(),
                cwd: "/repo".into(),
            },
        ]
    );
}

#[test]
fn recycled_baseline_pid_with_new_birth_token_is_not_excluded() {
    let baseline = vec![(200, 10)];
    let observed = vec![proc_entry(
        200,
        100,
        99,
        &["bash", "-lc", "npm test"],
        Some("/repo"),
    )];
    let selected = select_background_processes(100, &observed, &baseline, None, "/repo");
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].pid, 200);
    assert_eq!(selected[0].born, 99);
    assert_eq!(selected[0].command, "npm test");
}

#[test]
fn bare_shell_resolves_to_active_child_and_idle_bare_shell_is_skipped() {
    let observed = vec![
        proc_entry(400, 100, 10, &["pwsh.exe", "-NoProfile"], Some("/repo")),
        proc_entry(401, 100, 20, &["bash", "-l"], Some("/repo")),
        proc_entry(402, 401, 21, &["cargo", "test", "--lib"], Some("/repo")),
    ];
    let selected = select_background_processes(100, &observed, &[], None, "/repo");
    assert_eq!(
        selected,
        vec![SelectedBackgroundProcess {
            pid: 401,
            born: 20,
            command: "cargo test --lib".into(),
            cwd: "/repo".into(),
        }]
    );
}

#[test]
fn os_process_id_roundtrip_and_invalid_rejection() {
    let encoded = format_os_process_id(4242, 987654321);
    assert_eq!(encoded, "os:4242:987654321");
    assert_eq!(parse_os_process_id(&encoded), Some((4242, 987654321)));
    assert_eq!(parse_os_process_id("os:1:10"), None);
    assert_eq!(parse_os_process_id("os:0:10"), None);
    assert_eq!(parse_os_process_id("bridge:abc"), None);
    assert_eq!(parse_os_process_id("os:notapid:10"), None);
}

#[test]
fn exit_wait_cancel_signals_cleanly() {
    let cancel = ExitWaitCancel::new().expect("create cancel signal");
    cancel.cancel();
    // Repeated cancel must stay idempotent and panic-free.
    cancel.cancel();
}
