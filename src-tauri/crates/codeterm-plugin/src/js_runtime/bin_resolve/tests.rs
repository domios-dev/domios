use super::*;
use crate::js_runtime::PROCESS_TEST_LOCK as TEST_LOCK;

fn environment_with_paths(
    inherited: Option<&Path>,
    machine: EnvironmentValue,
    user: EnvironmentValue,
) -> BinSearchEnvironment {
    let inherited_path = inherited.map(|path| std::env::join_paths([path.as_os_str()]).unwrap());
    BinSearchEnvironment {
        inherited_path,
        machine_path: machine,
        user_path: user,
        pathext: Some(OsString::from(".COM;.EXE;.BAT;.CMD")),
    }
}

fn cached_count(bin: &str) -> usize {
    RESOLVED_BINS
        .lock()
        .keys()
        .filter(|key| key.binary == bin)
        .count()
}

#[test]
fn resolve_bin_probes_fallback_dir_when_not_on_path() {
    // A bin absent from PATH must still resolve through a fallback candidate
    // dir, which is the only rescue a GUI-launched daemon has.
    let name = format!("bwprobe_{}", uuid::Uuid::new_v4().simple());
    let dir = std::env::temp_dir().join(format!(
        "host_exec_resolve_{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    #[cfg(windows)]
    let bin_path = dir.join(format!("{name}.exe"));
    #[cfg(not(windows))]
    let bin_path = dir.join(&name);
    std::fs::write(&bin_path, b"#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&bin_path).unwrap().permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&bin_path, perm).unwrap();
    }

    let resolved = resolve_bin(&name, std::slice::from_ref(&dir));
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(
        resolved, bin_path,
        "resolve_bin must find a bin via the provided fallback dir, not just PATH"
    );
}

#[test]
fn resolve_bin_preserves_process_path_precedence() {
    let _guard = TEST_LOCK.lock();
    RESOLVED_BINS.lock().clear();
    #[cfg(windows)]
    let name = format!("host_exec_{}.cmd", uuid::Uuid::new_v4().simple());
    #[cfg(not(windows))]
    let name = format!("host_exec_{}", uuid::Uuid::new_v4().simple());

    let path_dir =
        std::env::temp_dir().join(format!("host_exec_path_{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&path_dir).unwrap();
    let expected = path_dir.join(&name);
    std::fs::write(&expected, b"#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&expected).unwrap().permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&expected, perm).unwrap();
    }

    let dir =
        std::env::temp_dir().join(format!("host_exec_decoy_{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    let decoy = dir.join(&name);
    std::fs::write(&decoy, b"#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&decoy).unwrap().permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&decoy, perm).unwrap();
    }

    let environment = environment_with_paths(
        Some(&path_dir),
        EnvironmentValue::Missing,
        EnvironmentValue::Value(std::env::join_paths([dir.as_os_str()]).unwrap()),
    );
    let resolved = probe_bin_with_environment(&name, &[], &environment).resolution;
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&path_dir);
    RESOLVED_BINS.lock().clear();
    assert_eq!(
        resolved,
        BinResolution::Found(expected),
        "the inherited process PATH must retain precedence over persisted PATH"
    );
}

/// The GUI-launched daemon that shipped `available:false` had no `HOME` at
/// all, so every home-relative candidate vanished and the ladder had
/// nothing left to try. Home must survive an unset `HOME`.
#[test]
fn well_known_bin_dirs_reach_user_local_bin_without_home() {
    let _guard = TEST_LOCK.lock();
    let mut process_env = codeterm_util::test_env::EnvGuard::lock();
    let home =
        std::env::temp_dir().join(format!("host_exec_home_{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(home.join(".local/bin")).unwrap();
    process_env.remove("HOME");
    process_env.set("USERPROFILE", &home);

    let dirs = well_known_bin_dirs();
    let _ = std::fs::remove_dir_all(&home);
    assert!(
        dirs.contains(&home.join(".local/bin")),
        "an unset HOME must not drop the home-relative candidates: {dirs:?}"
    );
}

/// The installed file had a platform suffix (`fixture.exe`); the probe only
/// ever looked for the bare name, so the rescue dir could not answer even when
/// it held the CLI.
#[test]
fn resolve_bin_probe_accepts_platform_executable_suffix() {
    let _guard = TEST_LOCK.lock();
    RESOLVED_BINS.lock().clear();
    let name = format!("bwext_{}", uuid::Uuid::new_v4().simple());
    let dir = std::env::temp_dir().join(format!("host_exec_ext_{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    #[cfg(windows)]
    let bin_path = dir.join(format!("{name}.exe"));
    #[cfg(not(windows))]
    let bin_path = dir.join(&name);
    std::fs::write(&bin_path, b"#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&bin_path).unwrap().permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&bin_path, perm).unwrap();
    }

    let environment = environment_with_paths(
        None,
        EnvironmentValue::Missing,
        EnvironmentValue::Value(std::env::join_paths([dir.as_os_str()]).unwrap()),
    );
    let resolved = probe_bin_with_environment(&name, &[], &environment).resolution;
    let _ = std::fs::remove_dir_all(&dir);
    RESOLVED_BINS.lock().clear();
    assert_eq!(
        resolved,
        BinResolution::Found(bin_path),
        "a probe dir holding the platform-suffixed executable must answer the bare bin"
    );
}

#[test]
fn explicit_and_relative_extensionless_paths_are_checked_directly() {
    let _guard = TEST_LOCK.lock();
    RESOLVED_BINS.lock().clear();
    let name = format!("bwdirect_{}", uuid::Uuid::new_v4().simple());
    let root = std::env::current_dir().unwrap().join(format!(
        "bin_resolve_paths_{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let absolute = root.join(&name);
    std::fs::write(&absolute, b"#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(windows)]
    let explicit_suffixed = root.join(format!("{name}.exe"));
    #[cfg(windows)]
    std::fs::write(&explicit_suffixed, b"MZ fixture bytes; never executed").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&absolute).unwrap().permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&absolute, perm).unwrap();
    }
    let environment =
        environment_with_paths(None, EnvironmentValue::Missing, EnvironmentValue::Missing);
    let explicit =
        probe_bin_with_environment(absolute.to_str().unwrap(), &[], &environment).resolution;
    #[cfg(windows)]
    let suffixed =
        probe_bin_with_environment(explicit_suffixed.to_str().unwrap(), &[], &environment)
            .resolution;
    let relative_input = absolute
        .strip_prefix(std::env::current_dir().unwrap())
        .unwrap()
        .to_path_buf();
    let relative =
        probe_bin_with_environment(relative_input.to_str().unwrap(), &[], &environment).resolution;
    let _ = std::fs::remove_dir_all(&root);
    RESOLVED_BINS.lock().clear();

    assert_eq!(explicit, BinResolution::Found(absolute));
    assert_eq!(relative, BinResolution::Found(relative_input));
    #[cfg(windows)]
    assert_eq!(suffixed, BinResolution::Found(explicit_suffixed));
}

#[cfg(windows)]
#[test]
fn windows_bare_names_ignore_extensionless_shims_for_path_and_rescue() {
    let _guard = TEST_LOCK.lock();
    RESOLVED_BINS.lock().clear();
    let name = format!("bwshim_{}", uuid::Uuid::new_v4().simple());
    let path_root = std::env::temp_dir().join(format!(
        "bin_resolve_shim_path_{}",
        uuid::Uuid::new_v4().simple()
    ));
    let rescue_root = std::env::temp_dir().join(format!(
        "bin_resolve_shim_rescue_{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&path_root).unwrap();
    std::fs::create_dir_all(&rescue_root).unwrap();

    let path_shim = path_root.join(&name);
    let path_cmd = path_root.join(format!("{name}.cmd"));
    std::fs::write(&path_shim, b"#!/bin/sh\nexit 0\n").unwrap();
    std::fs::write(&path_cmd, b"@echo off\r\nexit /b 0\r\n").unwrap();
    let path_environment = environment_with_paths(
        Some(&path_root),
        EnvironmentValue::Missing,
        EnvironmentValue::Missing,
    );
    let path_probe = probe_bin_with_environment(&name, &[], &path_environment);
    let path_cached = probe_bin_with_environment(&name, &[], &path_environment);

    let rescue_shim = rescue_root.join(&name);
    let rescue_exe = rescue_root.join(format!("{name}.exe"));
    std::fs::write(&rescue_shim, b"#!/bin/sh\nexit 0\n").unwrap();
    std::fs::write(&rescue_exe, b"MZ fixture bytes; never executed").unwrap();
    let rescue_environment =
        environment_with_paths(None, EnvironmentValue::Missing, EnvironmentValue::Missing);
    let rescue_probe = probe_bin_with_environment(
        &name,
        std::slice::from_ref(&rescue_root),
        &rescue_environment,
    );
    let rescue_cached = probe_bin_with_environment(
        &name,
        std::slice::from_ref(&rescue_root),
        &rescue_environment,
    );
    let cache_entries = cached_count(&name);

    let _ = std::fs::remove_dir_all(&path_root);
    let _ = std::fs::remove_dir_all(&rescue_root);
    RESOLVED_BINS.lock().clear();

    assert_eq!(path_probe.resolution, BinResolution::Found(path_cmd));
    assert_eq!(path_cached.resolution, path_probe.resolution);
    assert_eq!(rescue_probe.resolution, BinResolution::Found(rescue_exe));
    assert_eq!(rescue_cached.resolution, rescue_probe.resolution);
    assert_eq!(cache_entries, 2);
}

#[test]
fn non_runnable_candidates_are_not_reported_as_found() {
    let _guard = TEST_LOCK.lock();
    RESOLVED_BINS.lock().clear();
    let name = format!("bwnonrun_{}", uuid::Uuid::new_v4().simple());
    let root = std::env::temp_dir().join(format!(
        "bin_resolve_nonrun_{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(root.join(&name)).unwrap();
    let environment = environment_with_paths(
        Some(&root),
        EnvironmentValue::Missing,
        EnvironmentValue::Missing,
    );
    let probe = probe_bin_with_environment(&name, &[], &environment);
    #[cfg(unix)]
    let non_executable = {
        let file_name = format!("{name}_file");
        let file = root.join(&file_name);
        std::fs::write(&file, b"not executable").unwrap();
        let mut perm = std::fs::metadata(&file).unwrap().permissions();
        use std::os::unix::fs::PermissionsExt;
        perm.set_mode(0o644);
        std::fs::set_permissions(&file, perm).unwrap();
        probe_bin_with_environment(&file_name, &[], &environment).resolution
    };
    let _ = std::fs::remove_dir_all(&root);
    RESOLVED_BINS.lock().clear();

    assert_eq!(probe.resolution, BinResolution::Missing);
    #[cfg(unix)]
    assert_eq!(non_executable, BinResolution::Missing);
}

#[test]
fn an_earlier_path_candidate_takes_precedence_after_it_appears() {
    let _guard = TEST_LOCK.lock();
    RESOLVED_BINS.lock().clear();
    let name = format!("bwprecedence_{}", uuid::Uuid::new_v4().simple());
    let earlier = std::env::temp_dir().join(format!(
        "bin_resolve_earlier_{}",
        uuid::Uuid::new_v4().simple()
    ));
    let later = std::env::temp_dir().join(format!(
        "bin_resolve_later_{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&earlier).unwrap();
    std::fs::create_dir_all(&later).unwrap();
    #[cfg(windows)]
    let later_path = later.join(format!("{name}.exe"));
    #[cfg(not(windows))]
    let later_path = later.join(&name);
    std::fs::write(&later_path, b"#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&later_path).unwrap().permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&later_path, perm).unwrap();
    }
    let environment = environment_with_paths(
        None,
        EnvironmentValue::Value(
            std::env::join_paths([earlier.as_os_str(), later.as_os_str()]).unwrap(),
        ),
        EnvironmentValue::Missing,
    );
    let first = probe_bin_with_environment(&name, &[], &environment).resolution;

    #[cfg(windows)]
    let earlier_path = earlier.join(format!("{name}.exe"));
    #[cfg(not(windows))]
    let earlier_path = earlier.join(&name);
    std::fs::write(&earlier_path, b"#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&earlier_path).unwrap().permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&earlier_path, perm).unwrap();
    }
    let second = probe_bin_with_environment(&name, &[], &environment).resolution;
    let _ = std::fs::remove_dir_all(&earlier);
    let _ = std::fs::remove_dir_all(&later);
    RESOLVED_BINS.lock().clear();

    assert_eq!(first, BinResolution::Found(later_path));
    assert_eq!(second, BinResolution::Found(earlier_path));
}

#[test]
fn a_miss_is_reprobed_after_the_binary_appears_without_forget() {
    let _guard = TEST_LOCK.lock();
    RESOLVED_BINS.lock().clear();
    let name = format!("bwmemo_{}", uuid::Uuid::new_v4().simple());
    let dir =
        std::env::temp_dir().join(format!("host_exec_memo_{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();

    let missed = resolve_bin(&name, std::slice::from_ref(&dir));
    assert_eq!(missed, std::path::PathBuf::from(&name));
    assert_eq!(resolved_bin_path(&name), None);

    #[cfg(windows)]
    let bin_path = dir.join(format!("{name}.exe"));
    #[cfg(not(windows))]
    let bin_path = dir.join(&name);
    std::fs::write(&bin_path, b"#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&bin_path).unwrap().permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&bin_path, perm).unwrap();
    }

    let reprobed = resolve_bin(&name, std::slice::from_ref(&dir));
    let watched = resolved_bin_path_in_context(
        &name,
        &search_context(
            &name,
            &BinSearchEnvironment::current(),
            std::slice::from_ref(&dir),
        ),
    );
    let _ = std::fs::remove_dir_all(&dir);
    RESOLVED_BINS.lock().clear();
    assert_eq!(reprobed, bin_path);
    assert_eq!(watched, Some(bin_path));
}

#[test]
fn removing_a_found_file_invalidates_the_cached_available_result() {
    let _guard = TEST_LOCK.lock();
    RESOLVED_BINS.lock().clear();
    let name = format!("bwremoved_{}", uuid::Uuid::new_v4().simple());
    let dir = std::env::temp_dir().join(format!(
        "bin_resolve_removed_{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    #[cfg(windows)]
    let bin_path = dir.join(format!("{name}.exe"));
    #[cfg(not(windows))]
    let bin_path = dir.join(&name);
    std::fs::write(&bin_path, b"#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&bin_path).unwrap().permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&bin_path, perm).unwrap();
    }

    let environment = environment_with_paths(
        Some(&dir),
        EnvironmentValue::Missing,
        EnvironmentValue::Missing,
    );
    assert_eq!(
        probe_bin_with_environment(&name, &[], &environment).resolution,
        BinResolution::Found(bin_path.clone())
    );
    std::fs::remove_file(&bin_path).unwrap();
    assert_eq!(
        probe_bin_with_environment(&name, &[], &environment).resolution,
        BinResolution::Missing
    );
    let _ = std::fs::remove_dir_all(&dir);
    RESOLVED_BINS.lock().clear();
}

#[test]
fn persisted_path_finds_a_binary_missing_from_the_daemon_path() {
    let _guard = TEST_LOCK.lock();
    let name = format!("bwpersisted_{}", uuid::Uuid::new_v4().simple());
    let stale = std::env::temp_dir().join(format!(
        "bin_resolve_stale_{}",
        uuid::Uuid::new_v4().simple()
    ));
    let installed = std::env::temp_dir().join(format!(
        "bin_resolve_installed spaced_{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&stale).unwrap();
    std::fs::create_dir_all(&installed).unwrap();
    #[cfg(windows)]
    let expected = installed.join(format!("{name}.exe"));
    #[cfg(not(windows))]
    let expected = installed.join(&name);

    let missing_environment = environment_with_paths(
        Some(&stale),
        EnvironmentValue::Missing,
        EnvironmentValue::Missing,
    );
    assert_eq!(
        probe_bin_with_environment(&name, &[], &missing_environment).resolution,
        BinResolution::Missing
    );
    std::fs::write(&expected, b"#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&expected).unwrap().permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&expected, perm).unwrap();
    }

    let environment = environment_with_paths(
        Some(&stale),
        EnvironmentValue::Missing,
        EnvironmentValue::Value(std::env::join_paths([installed.as_os_str()]).unwrap()),
    );
    let probe = probe_bin_with_environment(&name, &[], &environment);
    assert_eq!(probe.resolution, BinResolution::Found(expected.clone()));
    assert_eq!(probe.searched, vec![stale.clone(), installed.clone()]);
    let _ = std::fs::remove_dir_all(expected.parent().unwrap());
    let _ = std::fs::remove_dir_all(&stale);
}

#[test]
fn unreadable_persisted_path_is_unknown_until_a_search_can_answer() {
    let _guard = TEST_LOCK.lock();
    let name = format!("bwunreadable_{}", uuid::Uuid::new_v4().simple());
    let stale = std::env::temp_dir().join(format!(
        "bin_resolve_unreadable_{}",
        uuid::Uuid::new_v4().simple()
    ));
    let installed = std::env::temp_dir().join(format!(
        "bin_resolve_recovered_{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&stale).unwrap();
    std::fs::create_dir_all(&installed).unwrap();
    #[cfg(windows)]
    let expected = installed.join(format!("{name}.exe"));
    #[cfg(not(windows))]
    let expected = installed.join(&name);
    std::fs::write(&expected, b"#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&expected).unwrap().permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&expected, perm).unwrap();
    }

    let unreadable = environment_with_paths(
        Some(&stale),
        EnvironmentValue::Missing,
        EnvironmentValue::Unreadable("fixture read failure".into()),
    );
    assert_eq!(
        probe_bin_with_environment(&name, &[], &unreadable).resolution,
        BinResolution::Unknown
    );
    let readable = environment_with_paths(
        Some(&stale),
        EnvironmentValue::Missing,
        EnvironmentValue::Value(std::env::join_paths([installed.as_os_str()]).unwrap()),
    );
    assert_eq!(
        probe_bin_with_environment(&name, &[], &readable).resolution,
        BinResolution::Found(expected.clone())
    );
    let _ = std::fs::remove_dir_all(expected.parent().unwrap());
    let _ = std::fs::remove_dir_all(&stale);
}

/// A ladder with no PATH to read never searched, so its miss is an
/// unanswered question. Collapsing it into "missing" is the defect.
#[test]
fn probe_reports_unknown_when_the_path_could_not_be_searched() {
    let _guard = TEST_LOCK.lock();
    RESOLVED_BINS.lock().clear();
    let name = format!("bwunk_{}", uuid::Uuid::new_v4().simple());
    let empty = std::env::temp_dir().join(format!(
        "bin_resolve_unknown_{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&empty).unwrap();

    let searchable = environment_with_paths(
        Some(&empty),
        EnvironmentValue::Missing,
        EnvironmentValue::Missing,
    );
    let searched = probe_bin_with_environment(&name, &[], &searchable);
    RESOLVED_BINS.lock().clear();
    let unsearchable = probe_bin_with_environment(
        &name,
        std::slice::from_ref(&empty),
        &BinSearchEnvironment {
            inherited_path: None,
            machine_path: EnvironmentValue::Missing,
            user_path: EnvironmentValue::Missing,
            pathext: Some(OsString::from(".COM;.EXE;.BAT;.CMD")),
        },
    );
    let _ = std::fs::remove_dir_all(&empty);
    RESOLVED_BINS.lock().clear();

    assert_eq!(searched.resolution, BinResolution::Missing);
    assert_eq!(
        unsearchable.resolution,
        BinResolution::Unknown,
        "a search that could not run must not report a definitive miss"
    );
}

/// An unknown that got memoized would freeze a machine into a wrong
/// capability answer for the whole process lifetime.
#[test]
fn unknown_is_never_memoized() {
    let _guard = TEST_LOCK.lock();
    RESOLVED_BINS.lock().clear();
    let name = format!("bwnomemo_{}", uuid::Uuid::new_v4().simple());
    let environment = BinSearchEnvironment {
        inherited_path: None,
        machine_path: EnvironmentValue::Missing,
        user_path: EnvironmentValue::Missing,
        pathext: Some(OsString::from(".COM;.EXE;.BAT;.CMD")),
    };
    assert_eq!(
        probe_bin_with_environment(&name, &[], &environment).resolution,
        BinResolution::Unknown
    );
    let memoized = cached_count(&name) != 0;
    RESOLVED_BINS.lock().clear();
    assert!(!memoized, "an unanswered probe must stay retryable");
}

#[test]
fn probe_reports_the_directories_it_was_willing_to_walk() {
    let _guard = TEST_LOCK.lock();
    RESOLVED_BINS.lock().clear();
    let name = format!("bwsearch_{}", uuid::Uuid::new_v4().simple());
    let dir = std::env::temp_dir().join(format!(
        "bin_resolve_search_{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&dir).unwrap();

    let probe = probe_bin_in(&name, std::slice::from_ref(&dir));
    let _ = std::fs::remove_dir_all(&dir);
    RESOLVED_BINS.lock().clear();
    assert!(
        probe.searched.contains(&dir),
        "the rescue dir must be nameable in a refusal: {:?}",
        probe.searched
    );
}

#[test]
fn resolve_bin_via_probe_returns_platform_candidate() {
    let _guard = TEST_LOCK.lock();
    RESOLVED_BINS.lock().clear();
    let name = format!("bwbase_{}", uuid::Uuid::new_v4().simple());
    let dir =
        std::env::temp_dir().join(format!("host_exec_base_{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    #[cfg(windows)]
    let bin_path = dir.join(format!("{name}.exe"));
    #[cfg(not(windows))]
    let bin_path = dir.join(&name);
    std::fs::write(&bin_path, b"#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&bin_path).unwrap().permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&bin_path, perm).unwrap();
    }

    let resolved = resolve_bin(&name, std::slice::from_ref(&dir));
    let _ = std::fs::remove_dir_all(&dir);
    RESOLVED_BINS.lock().clear();
    assert_eq!(
        resolved, bin_path,
        "a probe-dir resolution must return the platform candidate"
    );
}

#[test]
fn merged_search_path_preserves_precedence_and_deduplicates_entries() {
    let root = std::env::temp_dir().join(format!(
        "bin_resolve_merged_{}",
        uuid::Uuid::new_v4().simple()
    ));
    let inherited = root.join("inherited");
    let machine = root.join("machine");
    let user = root.join("user");
    let extra = root.join("extra");
    let environment = environment_with_paths(
        Some(&inherited),
        EnvironmentValue::Value(std::env::join_paths([&machine, &inherited]).unwrap()),
        EnvironmentValue::Value(std::env::join_paths([&user, &machine]).unwrap()),
    );
    let joined = merged_search_path(&environment, &[user.clone(), extra.clone()]).unwrap();
    let entries: Vec<PathBuf> = std::env::split_paths(&joined).collect();
    assert_eq!(entries, vec![inherited, machine, user, extra]);
}

#[test]
fn merged_search_path_with_extra_dirs_includes_declared_install_locations() {
    let custom = std::env::temp_dir().join(format!(
        "bin_resolve_declared_{}",
        uuid::Uuid::new_v4().simple()
    ));
    let joined = merged_search_path_with_extra_dirs(std::slice::from_ref(&custom)).unwrap();
    let entries: Vec<PathBuf> = std::env::split_paths(&joined).collect();
    assert!(
        entries.contains(&custom),
        "declared install dir must appear in merged search PATH: {entries:?}"
    );
}
