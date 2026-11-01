use codeterm_service_core::pty::manager::PtyManager;
use std::collections::HashMap;

#[test]
fn test_pty_resize() {
    let mut manager = PtyManager::new();

    #[cfg(target_os = "windows")]
    let cmd = Some("cmd.exe".to_string());
    #[cfg(not(target_os = "windows"))]
    let cmd = Some("/bin/sh".to_string());

    let (pty_id, _reader) = {
        let gate = manager.spawn_gate();
        codeterm_service_core::pty::manager::build_session(gate, cmd, None, 80, 24, HashMap::new())
            .map(|built| manager.register_session(None, built))
    }
    .unwrap();

    let result = manager.resize(&pty_id, 120, 40);
    assert!(result.is_ok(), "Failed to resize PTY: {:?}", result.err());

    let _ = manager.kill(&pty_id);
}

#[test]
fn test_pty_kill() {
    let mut manager = PtyManager::new();

    #[cfg(target_os = "windows")]
    let cmd = Some("cmd.exe".to_string());
    #[cfg(not(target_os = "windows"))]
    let cmd = Some("/bin/sh".to_string());

    let (pty_id, _reader) = {
        let gate = manager.spawn_gate();
        codeterm_service_core::pty::manager::build_session(gate, cmd, None, 80, 24, HashMap::new())
            .map(|built| manager.register_session(None, built))
    }
    .unwrap();

    let result = manager.kill(&pty_id);
    assert!(result.is_ok(), "Failed to kill PTY: {:?}", result.err());

    let result = manager.kill(&pty_id);
    assert!(result.is_err());
}

#[test]
fn test_pty_write_invalid_id() {
    let manager = PtyManager::new();
    let result = manager.write("nonexistent", b"hello");
    assert!(result.is_err());
}

#[test]
fn test_pty_resize_invalid_id() {
    let mut manager = PtyManager::new();
    let result = manager.resize("nonexistent", 80, 24);
    assert!(result.is_err());
}

#[test]
fn test_pty_multiple_instances() {
    let mut manager = PtyManager::new();

    #[cfg(target_os = "windows")]
    let cmd = Some("cmd.exe".to_string());
    #[cfg(not(target_os = "windows"))]
    let cmd = Some("/bin/sh".to_string());

    let (id1, _r1) = {
        let gate = manager.spawn_gate();
        codeterm_service_core::pty::manager::build_session(
            gate,
            cmd.clone(),
            None,
            80,
            24,
            HashMap::new(),
        )
        .map(|built| manager.register_session(None, built))
    }
    .unwrap();
    let (id2, _r2) = {
        let gate = manager.spawn_gate();
        codeterm_service_core::pty::manager::build_session(gate, cmd, None, 80, 24, HashMap::new())
            .map(|built| manager.register_session(None, built))
    }
    .unwrap();

    assert_ne!(id1, id2);

    assert!(manager.write(&id1, b"echo 1\r\n").is_ok());
    assert!(manager.write(&id2, b"echo 2\r\n").is_ok());

    let _ = manager.kill(&id1);
    let _ = manager.kill(&id2);
}
