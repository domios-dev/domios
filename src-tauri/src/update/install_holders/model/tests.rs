use super::*;

fn holder(pid: u32, parent_pid: Option<u32>, path: Option<&str>) -> Holder {
    Holder {
        pid,
        name: format!("process-{pid}"),
        executable: path.map(PathBuf::from),
        parent_pid,
        owned_tab: None,
    }
}

#[test]
fn standalone_installed_cli_is_owned_but_names_and_other_installs_are_not() {
    let clis = [PathBuf::from(r"C:\Domios\cli\codeterm.exe")];
    assert_eq!(
        classify_holder(
            &holder(2, Some(90), Some(r"\\?\c:\DOMIOS\cli\codeterm.exe")),
            1,
            &clis,
            &[]
        ),
        HolderDisposition::OwnedHelper
    );
    for path in [
        r"C:\Other\cli\codeterm.exe",
        r"C:\Domios\codeterm.exe",
        r"C:\Domios\cli\foreign.exe",
    ] {
        assert_eq!(
            classify_holder(&holder(2, Some(1), Some(path)), 1, &clis, &[]),
            HolderDisposition::Foreign
        );
    }
}

#[test]
fn consoles_and_embedders_require_both_exact_path_and_current_instance_parent() {
    let helpers = [
        PathBuf::from(r"C:\Domios\conpty\x64\OpenConsole.exe"),
        PathBuf::from(r"C:\Domios\codeterm-embed.exe"),
    ];
    for path in &helpers {
        let path = path.to_str().unwrap();
        assert_eq!(
            classify_holder(&holder(2, Some(1), Some(path)), 1, &[], &helpers),
            HolderDisposition::OwnedHelper
        );
        for parent in [Some(90), None] {
            assert_eq!(
                classify_holder(&holder(2, parent, Some(path)), 1, &[], &helpers),
                HolderDisposition::Foreign
            );
        }
    }
    assert_eq!(
        classify_holder(&holder(2, Some(1), None), 1, &[], &helpers),
        HolderDisposition::Foreign
    );
}

#[test]
fn updater_is_never_terminated_and_unknown_holders_are_foreign() {
    assert_eq!(
        classify_holder(&holder(1, None, None), 1, &[], &[]),
        HolderDisposition::UpdatingApp
    );
    assert_eq!(
        classify_holder(
            &holder(2, Some(1), Some(r"C:\Domios\foreign.exe")),
            1,
            &[],
            &[]
        ),
        HolderDisposition::Foreign
    );
}

#[test]
fn reused_pid_is_not_the_restart_manager_process() {
    assert!(same_process_identity((1, 2), (1, 2)));
    assert!(!same_process_identity((1, 2), (1, 3)));
    assert!(!same_process_identity((1, 2), (2, 2)));
}

#[test]
fn notice_names_every_foreign_holder_and_pid_and_retry() {
    let holders = [holder(22, None, None), holder(83, None, None)];
    assert_eq!(foreign_notice(&holders), "Update blocked by install-file holders: process-22 (PID 22); process-83 (PID 83). Close these processes and choose Retry.");
}

#[test]
fn pinned_job_membership_proves_ownership_even_with_unknown_image() {
    let mut member = holder(2, Some(90), None);
    member.owned_tab = Some("Tab Euler (tab-1)".into());
    assert_eq!(
        classify_holder(&member, 1, &[], &[]),
        HolderDisposition::OwnedTab
    );
}
