use super::*;
use crate::service::agent_pad::{AgentPad, AgentPadRole};

fn envelope() -> Envelope {
    let golden: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../codeterm-neighbors/test/golden-v2.json"
    )))
    .unwrap();
    serde_json::from_value(golden["envelope"].clone()).unwrap()
}

#[test]
fn own_pad_files_seal_with_existing_bounds_digests_and_no_plaintext_copy() {
    let temp = tempfile::tempdir().unwrap();
    let pad = AgentPad::create(temp.path(), "sender", AgentPadRole::General).unwrap();
    let bytes = vec![0xa5; files::MAX_FILE_BYTES - files::FILE_OVERHEAD];
    std::fs::write(pad.path().join("report.bin"), &bytes).unwrap();
    let before = pad.list().unwrap();
    let envelope = envelope();
    let sealed = seal_files(&envelope, vec!["report.bin".into(); 4], Some(pad.clone())).unwrap();
    assert_eq!(sealed.len(), 4);
    for (manifest, object) in sealed {
        assert_eq!(manifest.name, "report.bin");
        assert_eq!(manifest.ciphertext_size, files::MAX_FILE_BYTES);
        assert_eq!(
            manifest
                .open(&binding(&envelope), &object)
                .unwrap()
                .as_slice(),
            bytes
        );
        assert!(!object.windows(64).any(|chunk| chunk == &bytes[..64]));
    }
    assert_eq!(pad.list().unwrap(), before);
    assert_eq!(std::fs::read(pad.path().join("report.bin")).unwrap(), bytes);
}

#[tokio::test]
async fn bad_pad_attachments_refuse_before_any_upload_or_local_write() {
    let fixture =
        crate::test_support::fixture(crate::multiplexer::state::MultiplexerState::new().0);
    let pad = AgentPad::create(fixture.temp.path(), "sender", AgentPadRole::General).unwrap();
    let sibling = AgentPad::create(fixture.temp.path(), "other", AgentPadRole::General).unwrap();
    std::fs::write(pad.path().join("good.bin"), b"opaque file bytes").unwrap();
    std::fs::write(sibling.path().join("secret.bin"), b"foreign pad bytes").unwrap();
    let file = std::fs::File::create(pad.path().join("large.bin")).unwrap();
    file.set_len((files::MAX_FILE_BYTES - files::FILE_OVERHEAD + 1) as u64)
        .unwrap();
    drop(file);
    let before = pad.list().unwrap();
    let transport = FirebaseTransport {
        calls: Arc::new(super::super::super::send::tests::NoNetwork),
        project: "fixture".into(),
        uid: "owner".into(),
        installation_id: "machine".into(),
    };
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    for (paths, expected) in [
        (vec!["good.bin".into(); 5], Error::InvalidRequest),
        (
            vec!["good.bin".into(), "large.bin".into()],
            Error::File(files::refusal::Failure::too_large()),
        ),
        (
            vec![sibling
                .path()
                .join("secret.bin")
                .to_string_lossy()
                .into_owned()],
            Error::InvalidRequest,
        ),
        (vec!["../other/secret.bin".into()], Error::InvalidRequest),
        (vec!["good.bin:stream".into()], Error::InvalidRequest),
        (vec![".".into()], Error::InvalidRequest),
    ] {
        let error = fixture
            .service
            .prepare_neighbor_files(
                &transport,
                &metadata,
                &envelope(),
                Zeroizing::new(String::new()),
                paths,
                Some(pad.clone()),
            )
            .await
            .err()
            .unwrap();
        assert_eq!(error, expected);
    }
    assert_eq!(pad.list().unwrap(), before);
    assert!(metadata
        .file_entries(&envelope().friendship_id)
        .await
        .unwrap()
        .is_empty());
}

#[test]
fn symlinked_pad_files_and_directories_refuse_even_when_the_target_is_inside_pad() {
    let temp = tempfile::tempdir().unwrap();
    let pad = AgentPad::create(temp.path(), "sender", AgentPadRole::General).unwrap();
    std::fs::create_dir(pad.path().join("nested")).unwrap();
    std::fs::write(pad.path().join("nested/report.bin"), b"opaque bytes").unwrap();
    let mut paths = vec!["linked-dir/report.bin"];
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            pad.path().join("nested/report.bin"),
            pad.path().join("linked.bin"),
        )
        .unwrap();
        std::os::unix::fs::symlink(pad.path().join("nested"), pad.path().join("linked-dir"))
            .unwrap();
        paths.push("linked.bin");
    }
    #[cfg(windows)]
    {
        match std::os::windows::fs::symlink_file(
            pad.path().join("nested/report.bin"),
            pad.path().join("linked.bin"),
        ) {
            Ok(()) => paths.push("linked.bin"),
            Err(error) if error.raw_os_error() == Some(1314) => {
                eprintln!("file symlink capability unavailable (Windows privilege 1314); directory reparse refusal is exercised with a junction");
            }
            Err(error) => panic!("file symlink fixture failed: {error}"),
        }
        let mut command = std::process::Command::new("cmd");
        codeterm_util::process_util::no_window_std(&mut command);
        let output = command
            .args(["/C", "mklink", "/J"])
            .arg(pad.path().join("linked-dir"))
            .arg(pad.path().join("nested"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    for relative in paths {
        assert!(matches!(
            seal_files(&envelope(), vec![relative.into()], Some(pad.clone())),
            Err(Error::InvalidRequest)
        ));
    }
}
