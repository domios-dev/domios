use super::*;
use std::fs;

#[test]
fn inaccessible_process_entries_are_skipped_without_hiding_inspectable_writers() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("proc");
    fs::create_dir_all(root.join("1/fd")).unwrap();
    fs::create_dir_all(root.join("2/fd")).unwrap();
    fs::create_dir_all(root.join("2/fdinfo")).unwrap();
    let file = fixture.path().join("attachment");
    fs::write(&file, "complete attachment").unwrap();
    std::os::unix::fs::symlink(&file, root.join("2/fd/3")).unwrap();
    let source = fs::metadata(&file).unwrap();
    for code in [libc::EACCES, libc::ESRCH, libc::ENOENT] {
        let mut skipped = 0;
        fs::write(root.join("2/fdinfo/3"), "flags:\t0100000\n").unwrap();
        assert_eq!(
            ensure_no_writer_in(&root, &source, |path| {
                if path == root.join("1/fd") {
                    skipped += 1;
                    Err(std::io::Error::from_raw_os_error(code))
                } else {
                    fs::read_dir(path)
                }
            }),
            Ok(())
        );
        assert_eq!(skipped, 1);
        fs::write(root.join("2/fdinfo/3"), "flags:\t0100001\n").unwrap();
        assert_eq!(
            ensure_no_writer_in(&root, &source, |path| {
                if path == root.join("1/fd") {
                    Err(std::io::Error::from_raw_os_error(code))
                } else {
                    fs::read_dir(path)
                }
            }),
            Err(super::super::WRITING.into())
        );
    }
}

#[test]
fn unexpected_process_inspection_errors_are_refused() {
    let fixture = tempfile::tempdir().unwrap();
    fs::create_dir(fixture.path().join("1")).unwrap();
    let source = fs::metadata(fixture.path()).unwrap();
    let error = ensure_no_writer_in(fixture.path(), &source, |path| {
        if path.ends_with("fd") {
            Err(std::io::Error::from_raw_os_error(libc::EIO))
        } else {
            fs::read_dir(path)
        }
    })
    .unwrap_err();
    assert!(error.starts_with("Cannot verify attachment writer handles:"));
}
