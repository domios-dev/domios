use super::*;
use std::{
    io::{BufRead, Write},
    sync::Arc,
    time::Duration,
};
use windows_sys::Win32::System::JobObjects::*;

const HELPER: &str =
    "service::agent_runtime::process_boundary::windows::tests::external_conpty_peer";

#[test]
#[ignore = "Subprocess fixture invoked by retirement_captures_external_conpty_descendants"]
fn external_conpty_peer() {
    if std::env::var_os("DOMIOS_EXTERNAL_TREE_FIXTURE").is_none() {
        return;
    }
    let mut command = std::process::Command::new("node");
    command.arg(format!(
        "{}/../../../tests/fixtures/agent-runtime/escaped-tree.mjs",
        crate::test_support::fixture_manifest_dir().display()
    ));
    let mut child = conpty::Process::spawn(command).unwrap();
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    assert!(!job.is_null());
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags =
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK;
    assert_ne!(
        unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        },
        0
    );
    let handle = unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, child.pid()) };
    assert!(!handle.is_null());
    assert_ne!(unsafe { AssignProcessToJobObject(job, handle) }, 0);
    unsafe { CloseHandle(handle) };
    let output = child.output().unwrap();
    child.input().unwrap().write_all(b"go\r\n").unwrap();
    for line in std::io::BufReader::new(output).lines() {
        let line = line.unwrap();
        if let Some((_, json)) = line.split_once("fixture-tree:") {
            if let Some(end) = json.find(']') {
                let pids = serde_json::from_str::<Vec<u32>>(&json[..=end]).unwrap();
                println!("fixture-pids:{}", serde_json::to_string(&pids).unwrap());
                std::io::stdout().flush().unwrap();
                break;
            }
        }
    }
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
    child.exit(1).unwrap();
    unsafe { CloseHandle(job) };
}

#[tokio::test]
async fn retirement_captures_external_conpty_descendants() {
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", HELPER, "--ignored", "--nocapture"])
        .env("DOMIOS_EXTERNAL_TREE_FIXTURE", "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true);
    crate::process_util::no_window_tokio_suspended(&mut command);
    let mut root = command.spawn().unwrap();
    let job = Arc::new(crate::service::windows_job::Job::contain(&root).unwrap());
    let boundary = Arc::new(Boundary::new(root.id().unwrap(), job.clone()).unwrap());
    struct Cleanup(Arc<Boundary>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = self.0.retire();
        }
    }
    let _cleanup = Cleanup(boundary.clone());
    crate::service::windows_job::resume(&root).unwrap();
    use tokio::io::{AsyncBufReadExt, BufReader};
    let mut lines = BufReader::new(root.stdout.take().unwrap()).lines();
    let pids = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let line = lines
                .next_line()
                .await
                .unwrap()
                .expect("fixture exited before readiness");
            if let Some((_, json)) = line.split_once("fixture-pids:") {
                break serde_json::from_str::<Vec<u32>>(json).unwrap();
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(pids.len(), 2);
    let retained = descendants(&boundary).unwrap();
    let mut witnesses = Vec::new();
    let contained = job.process_ids().unwrap();
    let escaped = pids.iter().all(|pid| !contained.contains(pid));
    for pid in pids {
        let member = Member::retain(pid).unwrap().unwrap();
        assert!(!member.exited().unwrap());
        witnesses.push(member);
    }
    assert!(retained.len() >= 2);
    tokio::task::spawn_blocking(move || boundary.retire())
        .await
        .unwrap()
        .unwrap();
    for member in witnesses {
        assert!(
            member.exited().unwrap(),
            "escaped command survived retirement"
        );
    }
    assert!(escaped, "fixture command must escape runtime Job");
    root.wait().await.unwrap();
}
