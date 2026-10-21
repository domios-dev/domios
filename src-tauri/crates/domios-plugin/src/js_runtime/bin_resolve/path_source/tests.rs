use std::ffi::OsString;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;

use super::super::{probe_bin_from, BinResolution, RESOLVED_BINS};
use super::*;
use crate::js_runtime::PROCESS_TEST_LOCK as TEST_LOCK;

struct UserPathFixture {
    user_dirs: Mutex<Vec<PathBuf>>,
}

impl PathSource for UserPathFixture {
    fn environment(&self) -> BinSearchEnvironment {
        BinSearchEnvironment {
            inherited_path: None,
            machine_path: EnvironmentValue::Missing,
            user_path: EnvironmentValue::Value(
                std::env::join_paths(self.user_dirs.lock().iter()).unwrap(),
            ),
            pathext: Some(OsString::from(".COM;.EXE;.BAT;.CMD")),
        }
    }
}

fn scratch_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "bin_path_source_{label}_{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn install_executable(dir: &Path, name: &str) -> PathBuf {
    #[cfg(windows)]
    let path = dir.join(format!("{name}.exe"));
    #[cfg(not(windows))]
    let path = dir.join(name);
    std::fs::write(&path, b"#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

#[test]
fn a_dir_added_to_the_user_path_after_start_is_found() {
    let _guard = TEST_LOCK.lock();
    let name = format!("pathsrc_{}", uuid::Uuid::new_v4().simple());
    let original = scratch_dir("original");
    let source = UserPathFixture {
        user_dirs: Mutex::new(vec![original.clone()]),
    };
    let before = probe_bin_from(&name, &[], &source).resolution;

    let installed = scratch_dir("installed");
    let binary = install_executable(&installed, &name);
    source.user_dirs.lock().push(installed.clone());
    let after = probe_bin_from(&name, &[], &source).resolution;

    let _ = std::fs::remove_dir_all(&original);
    let _ = std::fs::remove_dir_all(&installed);
    RESOLVED_BINS.lock().clear();
    assert_eq!(before, BinResolution::Missing);
    assert_eq!(after, BinResolution::Found(binary));
}

#[test]
fn a_missing_answer_is_not_pinned_by_the_cache() {
    let _guard = TEST_LOCK.lock();
    let name = format!("pathsrc_{}", uuid::Uuid::new_v4().simple());
    let dir = scratch_dir("later");
    let source = UserPathFixture {
        user_dirs: Mutex::new(vec![dir.clone()]),
    };
    let before = probe_bin_from(&name, &[], &source).resolution;
    let binary = install_executable(&dir, &name);
    let after = probe_bin_from(&name, &[], &source).resolution;

    let _ = std::fs::remove_dir_all(&dir);
    RESOLVED_BINS.lock().clear();
    assert_eq!(before, BinResolution::Missing);
    assert_eq!(after, BinResolution::Found(binary));
}

#[test]
fn the_login_shell_path_is_read_between_markers_only() {
    let output =
        b"motd noise\n__DOMIOS_PATH_BEGIN__/opt/fixture/bin:/usr/bin__DOMIOS_PATH_END__trailing";
    assert_eq!(
        parse_marked_path(output).as_deref(),
        Some(&b"/opt/fixture/bin:/usr/bin"[..])
    );
    assert_eq!(
        parse_marked_path(b"__DOMIOS_PATH_BEGIN__/unterminated"),
        None
    );
    assert_eq!(
        parse_marked_path(b"__DOMIOS_PATH_BEGIN____DOMIOS_PATH_END__"),
        None
    );
    assert_eq!(parse_marked_path(b"no markers at all"), None);
}
