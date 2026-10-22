
use super::*;
use std::io::Write;

fn write(dir: &Path, rel: &str, content: &[u8]) {
    let abs = dir.join(rel);
    if let Some(parent) = abs.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let mut f = fs::File::create(abs).unwrap();
    f.write_all(content).unwrap();
}

#[test]
fn deterministic_across_runs() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "plugin.json", b"{\"id\":\"x\"}\n");
    write(dir.path(), "main.js", b"export default 1;\n");
    let h1 = hash_bundle(dir.path()).unwrap();
    let h2 = hash_bundle(dir.path()).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
}

#[test]
fn crlf_lf_equivalent_for_text_files() {
    let a = tempfile::tempdir().unwrap();
    write(a.path(), "main.js", b"line1\r\nline2\r\n");
    let b = tempfile::tempdir().unwrap();
    write(b.path(), "main.js", b"line1\nline2\n");
    assert_eq!(
        hash_bundle(a.path()).unwrap(),
        hash_bundle(b.path()).unwrap()
    );
}

#[test]
fn lone_cr_normalised() {
    let a = tempfile::tempdir().unwrap();
    write(a.path(), "main.js", b"line1\rline2\r");
    let b = tempfile::tempdir().unwrap();
    write(b.path(), "main.js", b"line1\nline2\n");
    assert_eq!(
        hash_bundle(a.path()).unwrap(),
        hash_bundle(b.path()).unwrap()
    );
}

#[test]
fn binary_files_not_normalised() {
    // A.png file containing \r\n bytes should NOT be normalised — bumping the count of \r bytes must change the hash.
    let a = tempfile::tempdir().unwrap();
    write(a.path(), "icon.png", b"\x89PNG\r\n\x1a\n");
    let b = tempfile::tempdir().unwrap();
    write(b.path(), "icon.png", b"\x89PNG\n\x1a\n");
    assert_ne!(
        hash_bundle(a.path()).unwrap(),
        hash_bundle(b.path()).unwrap()
    );
}

#[test]
fn dotfiles_and_skips_ignored() {
    let a = tempfile::tempdir().unwrap();
    write(a.path(), "main.js", b"x\n");

    let b = tempfile::tempdir().unwrap();
    write(b.path(), "main.js", b"x\n");
    write(b.path(), ".DS_Store", b"junk");
    write(b.path(), ".git/HEAD", b"ref: refs/heads/main\n");
    write(b.path(), "node_modules/foo/index.js", b"console.log(1)\n");
    write(b.path(), "target/debug/x", b"compiled");

    assert_eq!(
        hash_bundle(a.path()).unwrap(),
        hash_bundle(b.path()).unwrap()
    );
}

#[test]
fn nested_directory_order_independent() {
    // Filesystem walk may visit "z/a.js" before "a/z.js" depending on FS; explicit sort step must produce the same hash.
    let a = tempfile::tempdir().unwrap();
    write(a.path(), "a/file.js", b"a\n");
    write(a.path(), "z/file.js", b"z\n");

    let b = tempfile::tempdir().unwrap();
    write(b.path(), "z/file.js", b"z\n");
    write(b.path(), "a/file.js", b"a\n");

    assert_eq!(
        hash_bundle(a.path()).unwrap(),
        hash_bundle(b.path()).unwrap()
    );
}

#[test]
fn forward_slash_canonical_path_separator() {
    // hashed on Unix for identical content. Direct check: rename file to nested and verify hash depends on logical name, not raw OS separator.
    let a = tempfile::tempdir().unwrap();
    write(a.path(), "sub/file.js", b"x\n");
    let h = hash_bundle(a.path()).unwrap();

    // Compute expected hash from canonical form ourselves.
    let mut inner = Sha256::new();
    inner.update(b"sub/file.js");
    inner.update([0u8]);
    inner.update(b"x\n");
    let entry = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(entry);
    assert_eq!(h, hex_encode(&outer.finalize()));
}

#[test]
fn different_content_different_hash() {
    let a = tempfile::tempdir().unwrap();
    write(a.path(), "main.js", b"a");
    let b = tempfile::tempdir().unwrap();
    write(b.path(), "main.js", b"b");
    assert_ne!(
        hash_bundle(a.path()).unwrap(),
        hash_bundle(b.path()).unwrap()
    );
}

#[test]
fn missing_dir_errors() {
    let path = std::env::temp_dir().join("does-not-exist-codeterm-test");
    let _ = fs::remove_dir_all(&path);
    assert!(matches!(
        hash_bundle(&path),
        Err(HashError::NotADirectory(_))
    ));
}

#[test]
fn runtime_bundle_copy_excludes_development_files_and_preserves_hash() {
    let source = tempfile::tempdir().unwrap();
    let embedded = tempfile::tempdir().unwrap();
    let installed = tempfile::tempdir().unwrap();
    for (name, content) in [
        (
            "runtime-files.txt",
            "plugin.json\nplugin.js\nruntime-files.txt\nui/\nassets/\nREADME.md\n",
        ),
        ("plugin.json", "{\"id\":\"demo\"}"),
        ("plugin.js", "runtime"),
        ("ui/app.js", "ui"),
        ("assets/data.json", "{}"),
        ("README.md", "Guide"),
        ("tests/parser.test.js", "test"),
        ("fixtures/log.json", "fixture"),
        ("src/plugin.ts", "source"),
        ("ui/src/main.tsx", "source"),
        ("ui/app.test.js", "test"),
        ("ui/app.js.map", "source map"),
        ("package.json", "{}"),
        ("AGENTS.md", "procedure"),
        ("build.mjs", "build"),
    ] {
        write(source.path(), name, content.as_bytes());
    }
    assert_eq!(
        hash_bundle(source.path()).unwrap(),
        "73dfbc1b9a7318aee3b291a2c1aef82d12f9ab68b460f5b9eb094fc51542a985"
    );
    copy_runtime_bundle(source.path(), embedded.path()).unwrap();
    copy_runtime_bundle(embedded.path(), installed.path()).unwrap();
    let mut entries = Vec::new();
    collect_entries(installed.path(), installed.path(), &mut entries).unwrap();
    let mut names: Vec<_> = entries.iter().map(|(name, _)| name.as_str()).collect();
    names.sort();
    assert_eq!(
        names,
        [
            "README.md",
            "assets/data.json",
            "plugin.js",
            "plugin.json",
            "runtime-files.txt",
            "ui/app.js"
        ]
    );
    assert_eq!(
        hash_bundle(source.path()).unwrap(),
        hash_bundle(embedded.path()).unwrap()
    );
    assert_eq!(
        hash_bundle(source.path()).unwrap(),
        hash_bundle(installed.path()).unwrap()
    );
    write(
        source.path(),
        "tests/new.test.js",
        b"changed development payload",
    );
    assert_eq!(
        hash_bundle(source.path()).unwrap(),
        hash_bundle(installed.path()).unwrap()
    );
    write(installed.path(), "ui/app.js", b"tampered");
    assert_ne!(
        hash_bundle(source.path()).unwrap(),
        hash_bundle(installed.path()).unwrap()
    );
}
