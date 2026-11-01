use super::*;
use std::sync::Arc;

fn context(root: &Path) -> SurfaceCallCtx {
    let caller = root.join("caller");
    let owner = root.join("owner");
    std::fs::create_dir_all(&caller).unwrap();
    std::fs::create_dir_all(&owner).unwrap();
    let mut host = crate::test_support::RecordingSurfaceHost::default();
    host.pads.insert("caller".into(), caller);
    host.pads.insert("owner".into(), owner);
    SurfaceCallCtx {
        host: Arc::new(host),
        caller_tab_id: Some("caller".into()),
        owner_tab_id: Some("owner".into()),
        data_dir: root.to_path_buf(),
        actor: Some("Caller".into()),
    }
}
#[test]
fn output_file_is_written_to_the_callers_pad_and_refuses_escape_or_device_paths() {
    let root = tempfile::tempdir().unwrap();
    let ctx = context(root.path());
    let file = write(&ctx, "evidence/snapshot.json", "opaque payload").unwrap();
    assert_eq!(std::fs::read_to_string(file).unwrap(), "opaque payload");
    assert!(!root.path().join("owner/evidence").exists());
    for bad in [
        "../escape.txt",
        "NUL",
        "NUL.txt",
        "NUL .txt",
        "x:stream",
        "CON.txt",
    ] {
        assert!(write(&ctx, bad, "bad").is_err(), "{bad}");
    }
    assert!(write(
        &ctx,
        root.path().join("outside.txt").to_str().unwrap(),
        "bad"
    )
    .is_err());
}
#[tokio::test]
async fn snapshot_and_text_output_redact_before_persisting() {
    let root = tempfile::tempdir().unwrap();
    let ctx = context(root.path());
    let runtime = BrowserRuntime::new(
        codeterm_browser::BrowserHostConfig::under_data_dir(root.path()),
        "Agent",
        "tab",
        Arc::new(tokio::sync::OnceCell::new()),
        Default::default(),
    );
    runtime.remember_secret("opaque-private-token").unwrap();
    for file in ["snapshot.json", "page.txt"] {
        let path = runtime
            .output_verb(&ctx, Some(file.into()), "value opaque-private-token".into())
            .await
            .unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "value REDACTED");
    }
    assert_eq!(
        parse(&["--json", "--out", "snapshot.json"].map(String::from)).unwrap(),
        (vec!["--json".into()], Some("snapshot.json".into()))
    );
    assert!(parse(&["--out".into()]).is_err());
}
#[cfg(unix)]
#[test]
fn output_refuses_symlink_escape() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let ctx = context(root.path());
    std::os::unix::fs::symlink(outside.path(), root.path().join("caller/linked")).unwrap();
    assert!(write(&ctx, "linked/file.txt", "bad").is_err());
    assert!(!outside.path().join("file.txt").exists());
}
