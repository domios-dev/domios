use codeterm_lib::pty::manager::PtyManager;
use std::collections::HashMap;

#[test]
fn test_pty_spawn() {
    let mut manager = PtyManager::new();

    #[cfg(target_os = "windows")]
    let cmd = Some("cmd.exe".to_string());
    #[cfg(not(target_os = "windows"))]
    let cmd = Some("/bin/sh".to_string());

    let result = manager.spawn(cmd, None, 80, 24, HashMap::new());
    assert!(result.is_ok(), "Failed to spawn PTY: {:?}", result.err());

    let (pty_id, _reader) = result.unwrap();
    assert!(!pty_id.is_empty());

    // Clean up
    let _ = manager.kill(&pty_id);
}

#[test]
fn test_pty_write() {
    let mut manager = PtyManager::new();

    #[cfg(target_os = "windows")]
    let cmd = Some("cmd.exe".to_string());
    #[cfg(not(target_os = "windows"))]
    let cmd = Some("/bin/sh".to_string());

    let (pty_id, _reader) = manager.spawn(cmd, None, 80, 24, HashMap::new()).unwrap();

    let result = manager.write(&pty_id, b"echo hello\r\n");
    assert!(result.is_ok(), "Failed to write to PTY: {:?}", result.err());

    let _ = manager.kill(&pty_id);
}

#[test]
fn test_pty_resize() {
    let mut manager = PtyManager::new();

    #[cfg(target_os = "windows")]
    let cmd = Some("cmd.exe".to_string());
    #[cfg(not(target_os = "windows"))]
    let cmd = Some("/bin/sh".to_string());

    let (pty_id, _reader) = manager.spawn(cmd, None, 80, 24, HashMap::new()).unwrap();

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

    let (pty_id, _reader) = manager.spawn(cmd, None, 80, 24, HashMap::new()).unwrap();

    let result = manager.kill(&pty_id);
    assert!(result.is_ok(), "Failed to kill PTY: {:?}", result.err());

    // Double kill should fail
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
fn test_pty_spawn_with_env_vars() {
    let mut manager = PtyManager::new();
    let mut env_vars = HashMap::new();
    env_vars.insert("CODETERM_TEST".to_string(), "1".to_string());
    env_vars.insert("CODETERM_PANE_ID".to_string(), "test-pane".to_string());

    #[cfg(target_os = "windows")]
    let cmd = Some("cmd.exe".to_string());
    #[cfg(not(target_os = "windows"))]
    let cmd = Some("/bin/sh".to_string());

    let result = manager.spawn(cmd, None, 80, 24, env_vars);
    assert!(result.is_ok());

    let (pty_id, _reader) = result.unwrap();
    let _ = manager.kill(&pty_id);
}

#[test]
fn test_pty_multiple_instances() {
    let mut manager = PtyManager::new();

    #[cfg(target_os = "windows")]
    let cmd = Some("cmd.exe".to_string());
    #[cfg(not(target_os = "windows"))]
    let cmd = Some("/bin/sh".to_string());

    let (id1, _r1) = manager
        .spawn(cmd.clone(), None, 80, 24, HashMap::new())
        .unwrap();
    let (id2, _r2) = manager.spawn(cmd, None, 80, 24, HashMap::new()).unwrap();

    assert_ne!(id1, id2);

    // Both should be writable
    assert!(manager.write(&id1, b"echo 1\r\n").is_ok());
    assert!(manager.write(&id2, b"echo 2\r\n").is_ok());

    let _ = manager.kill(&id1);
    let _ = manager.kill(&id2);
}
