use super::*;
use std::io::{BufRead, BufReader, Read};
use std::process::Stdio;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn foreground_cleanup_reaps_child_and_joins_descendant_pipe_readers() {
    let _guard = crate::js_runtime::PROCESS_TEST_LOCK.lock();
    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("powershell");
        command.args([
            "-NoProfile", "-Command",
            r#"$p = Start-Process -FilePath powershell -ArgumentList @('-NoProfile','-Command','$null = ([System.Threading.ManualResetEvent]::new($false)).WaitOne()') -NoNewWindow -PassThru; [Console]::Out.WriteLine($p.Id); [Console]::Out.Flush(); $null = ([System.Threading.ManualResetEvent]::new($false)).WaitOne()"#,
        ]);
        command
    };
    #[cfg(unix)]
    let mut command = {
        // dash gives an async list /dev/null stdin even with `<&0`, so a stdin reader exits 0
        // on its own; `tail -f /dev/null` ends only when the cancellation SIGKILLs the group.
        let mut command = Command::new("sh");
        command.args(["-c", "tail -f /dev/null & echo $!; wait"]);
        command
    };
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = Owned::spawn(&mut command).unwrap();
    let stdout = child.stdout.take().unwrap();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (eof_tx, eof_rx) = mpsc::channel();
    child.add_reader(std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let ready = reader.read_line(&mut line).map(|_| line);
        let _ = ready_tx.send(ready);
        let mut rest = Vec::new();
        let result = reader.read_to_end(&mut rest);
        let _ = eof_tx.send(result);
    }));
    let line = ready_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("descendant must announce readiness")
        .unwrap();
    let descendant: u32 = line.trim().parse().expect("exact descendant pid");
    assert_ne!(descendant, child.id());
    #[cfg(windows)]
    assert!(child
        .target
        .job
        .process_ids()
        .unwrap()
        .contains(&descendant));
    let cancellation = std::sync::Arc::new(super::super::control::Process::default());
    let _scope = super::super::control::Scope::new(cancellation.clone());
    let registration = super::super::control::spawned(child.target());
    cancellation.cancel();
    let status = child.finish().expect("direct child must be reaped");
    assert!(!status.success());
    assert!(child.try_wait().unwrap().is_some());
    assert!(child.readers.is_empty());
    assert!(eof_rx
        .try_recv()
        .expect("joined reader must observe EOF")
        .is_ok());
    #[cfg(windows)]
    assert!(child.target.job.process_ids().unwrap().is_empty());
    drop(registration);
}
