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
