use super::*;
use serde_json::json;
#[test]
fn fake_index_refuses_malformed_identity_binary_and_oversize_manifest() {
    let manifest = json!({"name":"@fixture/agent","version":"1.2.3","bin":{"agent":"lib/bin.js"},"dist":{"tarball":"https://registry.npmjs.org/fixture.tgz","integrity":"sha512-opaque","unpackedSize":123}});
    assert!(validate_manifest(
        &serde_json::to_vec(&manifest).unwrap(),
        "@fixture/agent",
        "1.2.3"
    )
    .is_ok());
    for (field, value) in [
        ("/name", json!("foreign")),
        ("/version", json!("9.9.9")),
        ("/bin/agent", json!("../../escape")),
        ("/dist/unpackedSize", json!(MAX_ARCHIVE_BYTES + 1)),
        ("/dist/integrity", json!("unverified")),
    ] {
        let mut invalid = manifest.clone();
        *invalid.pointer_mut(field).unwrap() = value;
        assert!(validate_manifest(
            &serde_json::to_vec(&invalid).unwrap(),
            "@fixture/agent",
            "1.2.3"
        )
        .is_err());
    }
    assert!(validate_manifest(b"{}", "fixture", "1.2.3").is_err());
}

struct FakePort {
    corrupt: bool,
    calls: std::sync::atomic::AtomicUsize,
}
#[async_trait::async_trait]
impl InstallPort for FakePort {
    async fn download(&self, url: &str, max: usize) -> Result<Vec<u8>, String> {
        if url.ends_with("/1.2.3") {
            assert_eq!(max, 1024 * 1024);
            let integrity =
                base64::prelude::BASE64_STANDARD.encode(Sha512::digest(b"fake-package-bytes"));
            Ok(serde_json::to_vec(&json!({"name":"@fixture/agent","version":"1.2.3","bin":{"agent":"lib/bin.js"},"dist":{"tarball":"https://registry.npmjs.org/fixture.tgz","integrity":format!("sha512-{integrity}"),"unpackedSize":18}})).unwrap())
        } else {
            assert_eq!(url, "https://registry.npmjs.org/fixture.tgz");
            assert_eq!(max, MAX_ARCHIVE_BYTES);
            Ok(if self.corrupt {
                b"bad".to_vec()
            } else {
                b"fake-package-bytes".to_vec()
            })
        }
    }
    async fn package_manager(&self, target: &Path, archive: &Path) -> Result<PathBuf, String> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(std::fs::read(archive).unwrap(), b"fake-package-bytes");
        let script = target.join("node_modules/@fixture/agent/lib/bin.js");
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        std::fs::write(script, b"fixture-script").unwrap();
        let node = target.join("node");
        std::fs::write(&node, b"fixture-node").unwrap();
        Ok(node)
    }
}
#[tokio::test]
async fn fake_download_and_package_manager_install_only_verified_archive() {
    let root = tempfile::tempdir().unwrap();
    let port = FakePort {
        corrupt: false,
        calls: std::sync::atomic::AtomicUsize::new(0),
    };
    let (command, checksum) = install_using(root.path(), "@fixture/agent", "1.2.3", &port)
        .await
        .unwrap();
    assert_eq!(port.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(command.args.len(), 1);
    assert!(Path::new(&command.args[0]).is_file());
    assert_eq!(checksum.len(), 64);
    let root = tempfile::tempdir().unwrap();
    let port = FakePort {
        corrupt: true,
        calls: std::sync::atomic::AtomicUsize::new(0),
    };
    assert!(install_using(root.path(), "@fixture/agent", "1.2.3", &port)
        .await
        .is_err());
    assert_eq!(port.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}
