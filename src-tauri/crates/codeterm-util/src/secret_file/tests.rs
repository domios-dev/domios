use super::*;

#[test]
fn in_memory_round_trip() {
    let kc = SecretFile::in_memory("test");
    assert!(!kc.is_available());
    assert_eq!(kc.availability_reason(), Some("test"));
    assert!(!kc.has("foo").unwrap());

    kc.set("foo", "bar").unwrap();
    assert_eq!(kc.get("foo").unwrap(), "bar");
    assert!(kc.has("foo").unwrap());

    kc.delete("foo").unwrap();
    assert!(matches!(kc.get("foo"), Err(SecretFileError::NotFound)));
    assert!(!kc.has("foo").unwrap());

    // Delete is idempotent.
    kc.delete("foo").unwrap();
}

#[test]
fn in_memory_overwrite_is_last_write_wins() {
    let kc = SecretFile::in_memory("test");
    kc.set("k", "v1").unwrap();
    kc.set("k", "v2").unwrap();
    assert_eq!(kc.get("k").unwrap(), "v2");
}

#[test]
fn unreadable_secrets_file_is_never_replaced_by_a_single_entry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".secrets");
    let unreadable = vec![0xff, 0xfe, b'{', 0x80];
    std::fs::write(&path, &unreadable).unwrap();

    assert!(write_file_entry(&path, "k", Some("v")).is_err());
    assert!(write_file_entry(&path, "k", None).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), unreadable);
}

#[test]
fn unparseable_secrets_file_is_never_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".secrets");
    std::fs::write(&path, "{\"a\": \"trunc").unwrap();

    assert!(write_file_entry(&path, "k", Some("v")).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"a\": \"trunc");
}

#[test]
fn explicit_data_root_uses_host_plugin_bucket_format() {
    let dir = tempfile::tempdir().unwrap();
    let cli = plugin_secret_bucket(dir.path(), "demo");
    cli.set("API_TOKEN", "opaque-value").unwrap();
    let host = plugin_secret_bucket(dir.path(), "demo");
    assert!(host.has("API_TOKEN").unwrap());
    assert!(host.get("API_TOKEN").unwrap() == "opaque-value");
    cli.set("API_TOKEN", "replacement").unwrap();
    let host = plugin_secret_bucket(dir.path(), "demo");
    assert!(host.get("API_TOKEN").unwrap() == "replacement");
}

#[test]
fn failed_persistence_does_not_update_the_shadow_value() {
    let root = tempfile::tempdir().unwrap();
    let store = SecretFile::new_at(root.path(), "test");
    store.set("key", "committed").unwrap();
    std::fs::write(root.path().join(".secrets"), "damaged").unwrap();
    assert!(store.set("key", "uncommitted").is_err());
    assert_eq!(store.get("key").unwrap(), "committed");
    assert!(store.delete("key").is_err());
    assert_eq!(store.get("key").unwrap(), "committed");
}

#[test]
fn cross_process_secret_writer_child() {
    use std::io::{BufRead, Write};
    let Some(root) = std::env::var_os("CODETERM_SECRET_LOCK_TEST_ROOT") else {
        return;
    };
    let path = std::path::Path::new(&root).join(".secrets");
    let lock = open_store_lock(&path).unwrap();
    assert!(matches!(
        lock.try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));
    println!("\nSECRET_LOCK_BLOCKED");
    std::io::stdout().flush().unwrap();
    let mut command = String::new();
    assert!(std::io::stdin().lock().read_line(&mut command).unwrap() > 0);
    assert_eq!(command.trim(), "WRITE");
    write_file_entry(&path, "codeterm.plugin.fixture:key", Some("plugin-value")).unwrap();
}

#[test]
fn concurrent_plugin_write_cannot_drop_the_instance_entry() {
    use std::io::{BufRead, Write};
    use std::process::{Command, Stdio};
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(".secrets");
    let lock = open_store_lock(&path).unwrap();
    lock.lock().unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "secret_file::tests::cross_process_secret_writer_child",
            "--nocapture",
        ])
        .env("CODETERM_SECRET_LOCK_TEST_ROOT", root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut output = std::io::BufReader::new(child.stdout.take().unwrap());
    loop {
        let mut line = String::new();
        assert!(
            output.read_line(&mut line).unwrap() > 0,
            "child exited before confirming exclusion"
        );
        if line.trim() == "SECRET_LOCK_BLOCKED" {
            break;
        }
    }
    write_locked_file_entry(&path, "domios.instance:api_token", Some("instance-value")).unwrap();
    drop(lock);
    child.stdin.as_mut().unwrap().write_all(b"WRITE\n").unwrap();
    child.stdin.as_mut().unwrap().flush().unwrap();
    assert!(child.wait().unwrap().success());
    let fresh = SecretFile::new_at(root.path(), "domios.instance");
    assert_eq!(fresh.get("api_token").unwrap(), "instance-value");
    assert_eq!(
        SecretFile::new_at(root.path(), "codeterm.plugin.fixture")
            .get("key")
            .unwrap(),
        "plugin-value"
    );
}

#[test]
fn store_contention_returns_busy_before_waiting_for_the_shadow_mutex() {
    let root = tempfile::tempdir().unwrap();
    let store = SecretFile::new_at(root.path(), "test");
    store.set("key", "committed").unwrap();
    let lock = lock_store(&root.path().join(".secrets")).unwrap();
    let shadow = store.shadow.lock();
    let writer = store.clone();
    let (sent, received) = std::sync::mpsc::channel();
    let child = std::thread::spawn(move || {
        sent.send((writer.set("key", "uncommitted"), writer.delete("key")))
            .unwrap();
    });
    let (set, delete) = received
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("contended writes must return without waiting for the shadow mutex");
    assert!(matches!(set, Err(SecretFileError::Busy)));
    assert!(matches!(delete, Err(SecretFileError::Busy)));
    drop(shadow);
    drop(lock);
    child.join().unwrap();
    assert_eq!(store.get("key").unwrap(), "committed");
    assert!(crate::private_file::owner_only(&root.path().join(".secrets.lock")).unwrap());
}

#[test]
fn a_cache_miss_cannot_overwrite_a_concurrent_set_or_delete() {
    for replacement in [Some("new"), None] {
        let root = tempfile::tempdir().unwrap();
        let store = SecretFile::new_at(root.path(), "test");
        write_file_entry(&root.path().join(".secrets"), "test:key", Some("old")).unwrap();
        let reader = store.clone();
        let (entered, reading) = std::sync::mpsc::channel();
        let (release, proceed) = std::sync::mpsc::channel();
        let get = std::thread::spawn(move || {
            reader.get_with_reader("key", |path, account| {
                let value = read_file_entry(path, account)?;
                entered.send(()).unwrap();
                proceed.recv().unwrap();
                Ok(value)
            })
        });
        reading
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        assert!(
            store.shadow.try_lock().is_none(),
            "a cache miss retains its shadow guard"
        );
        let writer = store.clone();
        let update = std::thread::spawn(move || match replacement {
            Some(value) => writer.set("key", value),
            None => writer.delete("key"),
        });
        release.send(()).unwrap();
        assert_eq!(get.join().unwrap().unwrap(), "old");
        update.join().unwrap().unwrap();
        match replacement {
            Some(value) => assert_eq!(store.get("key").unwrap(), value),
            None => assert!(matches!(store.get("key"), Err(SecretFileError::NotFound))),
        }
    }
}
