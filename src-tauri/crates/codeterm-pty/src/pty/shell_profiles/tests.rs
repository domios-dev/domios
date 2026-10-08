use super::*;

fn wsl() -> ShellProfile {
    ShellProfile {
        id: "wsl:Ubuntu".into(),
        name: "WSL: Ubuntu".into(),
        executable: "wsl.exe".into(),
        args: vec!["--distribution".into(), "Ubuntu".into()],
        wsl_distro: Some("Ubuntu".into()),
    }
}

#[test]
fn wsl_list_decodes_utf16_with_and_without_bom() {
    let text = "Ubuntu\r\nDebian\r\nUbuntu\r\n  \r\nopenSUSE-测试\r\n";
    let bytes: Vec<_> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    for bytes in [bytes.clone(), [vec![0xff, 0xfe], bytes].concat()] {
        assert_eq!(
            detection::parse_wsl_distros(&bytes),
            ["Ubuntu", "Debian", "openSUSE-测试"]
        );
    }
    assert_eq!(
        detection::parse_wsl_distros(b"Ubuntu\r\nDebian\n"),
        ["Ubuntu", "Debian"]
    );
    assert!(detection::parse_wsl_distros(b"\r\n").is_empty());
}

#[test]
fn unix_shells_filter_availability_and_deduplicate_ids() {
    let profiles = detection::parse_unix_shells(
        "# shells\n/bin/zsh\n/bin/sh\n/bin/bash\n/usr/bin/bash\n/bin/fish\nrelative\n",
        |p| p != "/bin/fish",
    );
    assert_eq!(
        profiles.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        ["zsh", "bash"]
    );
    assert_eq!(profiles[1].executable, "/bin/bash");
}

#[test]
fn windows_drive_mapping_preserves_spaces_and_unicode() {
    for (host, linux) in [
        (r"C:\Users\Test User\代码", "/mnt/c/Users/Test User/代码"),
        ("d:/repo", "/mnt/d/repo"),
        ("Z:/", "/mnt/z/"),
    ] {
        assert_eq!(windows_to_wsl(host, "Ubuntu").unwrap(), linux);
        assert_eq!(
            wsl_to_windows(linux, "Ubuntu").unwrap().to_lowercase(),
            host.replace('/', "\\").to_lowercase()
        );
    }
}

#[test]
fn wsl_native_paths_map_to_distribution_unc() {
    assert_eq!(
        windows_to_wsl(r"\\?\C:\repo", "Ubuntu").unwrap(),
        "/mnt/c/repo"
    );
    assert_eq!(
        wsl_to_windows("/home/me/repo", "Ubuntu").unwrap(),
        r"\\wsl.localhost\Ubuntu\home\me\repo"
    );
    assert_eq!(
        wsl_to_windows("/mnt/cache", "Ubuntu").unwrap(),
        r"\\wsl.localhost\Ubuntu\mnt\cache"
    );
    assert_eq!(wsl_to_windows("/mnt/c", "Ubuntu").unwrap(), r"C:\");
    for path in [r"\\wsl.localhost\Ubuntu\home\me", r"\\wsl$\Ubuntu\home\me"] {
        assert_eq!(windows_to_wsl(path, "Ubuntu").unwrap(), "/home/me");
    }
}

#[test]
fn mapping_rejects_relative_foreign_unc_and_invalid_distro() {
    for path in [
        "relative",
        "C:relative",
        r"\\server\share",
        r"\\wsl.localhost\Debian\home",
        r"\\wsl.localhost\Ubuntu-other\home",
    ] {
        assert!(windows_to_wsl(path, "Ubuntu").is_err(), "{path}");
    }
    for distro in ["", "..", "../Ubuntu", "Ubuntu\\repo"] {
        assert!(windows_to_wsl("C:/repo", distro).is_err());
        assert!(wsl_to_windows("/home", distro).is_err());
    }
    assert!(wsl_to_windows("relative", "Ubuntu").is_err());
}

#[test]
fn wsl_launch_uses_arguments_without_a_host_cwd() {
    let launch = wsl().launch(Some(r"C:\a b\repo")).unwrap();
    assert_eq!(launch.executable, "wsl.exe");
    assert_eq!(
        launch.args,
        ["--distribution", "Ubuntu", "--cd", "/mnt/c/a b/repo"]
    );
    assert_eq!(launch.cwd, None);
    assert_eq!(wsl().launch(None).unwrap().args.last().unwrap(), "~");
}

#[test]
fn native_launch_preserves_executable_arguments_and_cwd() {
    let profile = ShellProfile {
        id: "bash".into(),
        name: "bash".into(),
        executable: "/bin/bash".into(),
        args: vec!["-l".into()],
        wsl_distro: None,
    };
    let launch = profile.launch(Some("/tmp/a b")).unwrap();
    assert_eq!(launch.args, ["-l"]);
    assert_eq!(launch.cwd.as_deref(), Some("/tmp/a b"));
    assert_eq!(profile.host_cwd("/tmp/a b").unwrap(), "/tmp/a b");
}

#[test]
fn cached_detection_changes_only_on_explicit_refresh() {
    let catalog = ShellProfileCatalog::default();
    let first = catalog.load_with(false, || vec![wsl()]);
    let cached = catalog.load_with(false, || panic!("cached reads must not detect"));
    assert!(Arc::ptr_eq(&first, &cached));
    let refreshed = catalog.load_with(true, Vec::new);
    assert!(refreshed.is_empty());
    assert!(!Arc::ptr_eq(&first, &refreshed));
    assert_eq!(first.len(), 1);
    assert!(Arc::ptr_eq(
        &refreshed,
        &catalog.load_with(false, || panic!("refresh must replace the cache"))
    ));
}

#[test]
fn concurrent_initial_reads_share_one_detection() {
    let catalog = Arc::new(ShellProfileCatalog::default());
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let catalog = catalog.clone();
            let calls = calls.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                catalog.load_with(false, || {
                    calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    vec![wsl()]
                })
            })
        })
        .collect();
    barrier.wait();
    let profiles: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert!(Arc::ptr_eq(&profiles[0], &profiles[1]));
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn detection_never_holds_the_snapshot_lock() {
    let catalog = ShellProfileCatalog::default();
    catalog.load_with(false, || {
        assert!(catalog.profiles.try_read().is_some());
        vec![wsl()]
    });
    catalog.load_with(true, || {
        assert_eq!(
            catalog.profiles.try_read().unwrap().as_ref().unwrap().len(),
            1
        );
        Vec::new()
    });
}

#[test]
fn selection_preserves_automatic_and_prefers_requested_profile() {
    let profiles = vec![wsl()];
    assert_eq!(select(&profiles, None, None).unwrap(), None);
    assert_eq!(
        select(&profiles, None, Some("wsl:Ubuntu")).unwrap(),
        Some(wsl())
    );
    assert_eq!(
        select(&profiles, Some("wsl:Ubuntu"), Some("unavailable")).unwrap(),
        Some(wsl())
    );
    assert!(select(&profiles, Some("unavailable"), None).is_err());
}
