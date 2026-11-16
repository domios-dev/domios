use super::*;

const BROWSER_ID: &str = "5c1a3b9e-8f0d-4f8e-9f41-2b1c0e6d7a10";

fn owned_args() -> Vec<String> {
    launch_args(&LaunchSpec {
        profile_dir: Path::new("/data/Domios Browser/profiles/work"),
        width: 960,
        height: 640,
        initial_url: "about:blank",
    })
}

#[test]
fn browser_launch_macos_app_avoids_recents_and_existing_instances() {
    let args = owned_args();
    let spec = macos_app_launch(
        HostOs::MacOs,
        Path::new("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
        &args,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        spec,
        MacOsAppLaunch {
            bundle: PathBuf::from("/Applications/Google Chrome.app"),
            profile_dir: PathBuf::from("/data/Domios Browser/profiles/work"),
            adds_to_recent_items: false,
            creates_new_instance: true,
            activates: false,
        }
    );
    assert!(args.iter().any(|arg| arg == "--headless=new"));
    assert!(args.iter().any(|arg| arg == "--remote-debugging-port=0"));
}

#[test]
fn browser_launch_windows_linux_and_standalone_binaries_keep_direct_launch() {
    let args = owned_args();
    for (os, binary) in [
        (
            HostOs::Windows,
            "C:/Program Files/Google/Chrome/Application/chrome.exe",
        ),
        (HostOs::Linux, "/usr/bin/google-chrome"),
        (
            HostOs::Windows,
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        ),
        (
            HostOs::Linux,
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        ),
        (HostOs::MacOs, "/opt/chrome-headless-shell"),
        (HostOs::MacOs, "/custom/Contents/MacOS/chrome"),
    ] {
        assert_eq!(
            macos_app_launch(os, Path::new(binary), &args).unwrap(),
            None
        );
    }
}

#[test]
fn browser_launch_app_requires_owned_isolated_profile() {
    let binary = Path::new("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome");
    for args in [
        Vec::new(),
        vec![OWNERSHIP_FLAG.into()],
        vec!["--user-data-dir=/data/foreign".into()],
        vec![OWNERSHIP_FLAG.into(), "--user-data-dir=".into()],
        vec![
            OWNERSHIP_FLAG.into(),
            "--user-data-dir=/one".into(),
            "--user-data-dir=/two".into(),
        ],
    ] {
        assert!(macos_app_launch(HostOs::MacOs, binary, &args).is_err());
    }
}

#[test]
fn browser_active_port_requires_complete_loopback_browser_endpoint() {
    assert_eq!(
        active_port_endpoint(&format!("53817\n/devtools/browser/{BROWSER_ID}")),
        Some(format!(
            "ws://127.0.0.1:53817/devtools/browser/{BROWSER_ID}"
        ))
    );
    for contents in [
        "",
        "53817\n",
        "53817\n/devtools/browser/5c1a3b9e",
        "0\n/devtools/browser/5c1a3b9e",
        "65536\n/devtools/browser/5c1a3b9e",
        "53817\nws://192.168.1.4:9222/devtools/browser/x",
        "53817\n/devtools/page/5c1a3b9e-8f0d-4f8e-9f41-2b1c0e6d7a10",
    ] {
        assert_eq!(active_port_endpoint(contents), None, "{contents:?}");
    }
}

const READY: &str =
    "DevTools listening on ws://127.0.0.1:53817/devtools/browser/5c1a3b9e-8f0d-4f8e-9f41-2b1c0e6d7a10";

#[test]
fn browser_launch_args_carry_hygiene_flags_and_profile() {
    let profile = Path::new("/data/browsers/sessions/abc");
    let args = launch_args(&LaunchSpec {
        profile_dir: profile,
        width: 1600,
        height: 900,
        initial_url: "",
    });
    for flag in [
        "--headless=new",
        "--remote-debugging-port=0",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-sync",
        "--disable-extensions",
        "--disable-component-extensions-with-background-pages",
        "--disable-background-networking",
        "--password-store=basic",
        "--use-mock-keychain",
        "--disable-features=Translate,CalculateNativeWinOcclusion",
        "--disable-blink-features=AutomationControlled",
        "--window-size=1600,900",
    ] {
        assert!(args.iter().any(|arg| arg == flag), "missing {flag}");
    }
    assert!(args.contains(&format!("--user-data-dir={}", profile.display())));
    assert_eq!(args.last().map(String::as_str), Some("about:blank"));
}

#[test]
fn browser_launch_args_end_with_initial_url_when_given() {
    let args = launch_args(&LaunchSpec {
        profile_dir: Path::new("p"),
        width: 0,
        height: 0,
        initial_url: "http://127.0.0.1:5173/",
    });
    assert_eq!(
        args.last().map(String::as_str),
        Some("http://127.0.0.1:5173/")
    );
    assert!(args.iter().any(|arg| arg == "--window-size=1,1"));
}

#[test]
fn browser_devtools_line_parses_endpoint() {
    assert_eq!(
        parse_devtools_line(READY).as_deref(),
        Some("ws://127.0.0.1:53817/devtools/browser/5c1a3b9e-8f0d-4f8e-9f41-2b1c0e6d7a10")
    );
    assert_eq!(parse_devtools_line("DevTools listening on http://x"), None);
    assert_eq!(
        parse_devtools_line("[1008/101010.123:ERROR:gpu] nope"),
        None
    );
}

#[test]
fn browser_devtools_listener_finds_endpoint_among_real_stderr_noise() {
    let mut listener = DevToolsListener::default();
    let stderr = format!(
        "[24312:23440:1008/120001.512:ERROR:cert_verify_proc_builtin.cc(1052)] CertVerifyProcBuiltin failed\r\n\r\n{READY}\r\n[24312:1008/120001.700:WARNING:gpu_init.cc(456)] fallback\r\n"
    );
    assert_eq!(
        listener.push(stderr.as_bytes()).as_deref(),
        parse_devtools_line(READY).as_deref()
    );
}

#[test]
fn browser_devtools_listener_joins_partial_chunks() {
    let mut listener = DevToolsListener::default();
    let bytes = format!("noise line\n{READY}\r\n");
    let (a, rest) = bytes.as_bytes().split_at(17);
    let (b, c) = rest.split_at(30);
    assert_eq!(listener.push(a), None);
    assert_eq!(listener.push(b), None);
    assert!(listener.push(c).unwrap().ends_with("2b1c0e6d7a10"));
}

#[test]
fn browser_devtools_listener_strips_crlf_split_between_chunks() {
    let mut listener = DevToolsListener::default();
    assert_eq!(listener.push(format!("{READY}\r").as_bytes()), None);
    assert!(listener.push(b"\n").unwrap().ends_with("2b1c0e6d7a10"));
}

#[test]
fn browser_devtools_listener_accepts_unterminated_last_line_on_finish() {
    let mut listener = DevToolsListener::default();
    assert_eq!(listener.push(READY.as_bytes()), None);
    assert!(listener.finish().is_some());
}

#[test]
fn browser_devtools_listener_keeps_bounded_tail_for_diagnostics() {
    let mut listener = DevToolsListener::default();
    for index in 0..20 {
        listener.push(format!("line {index}\r\n").as_bytes());
    }
    let tail = listener.tail();
    assert!(tail.starts_with("line 12"));
    assert!(tail.ends_with("line 19"));
}

#[test]
fn browser_devtools_listener_bounds_an_endless_line() {
    let mut listener = DevToolsListener::default();
    listener.push(&vec![b'x'; MAX_PENDING_LINE * 2]);
    assert_eq!(listener.pending.len(), MAX_PENDING_LINE);
}

#[test]
fn browser_endpoint_validation_accepts_only_loopback() {
    assert!(validate_loopback_endpoint("ws://127.0.0.1:9222/devtools/browser/x", &["ws"]).is_ok());
    assert!(validate_loopback_endpoint("ws://localhost:9222/x", &["ws"]).is_ok());
    assert!(validate_loopback_endpoint("ws://[::1]:9222/x", &["ws"]).is_ok());
    assert!(validate_loopback_endpoint("ws://192.168.1.4:9222/x", &["ws"]).is_err());
    assert!(validate_loopback_endpoint("wss://127.0.0.1:9222/x", &["ws"]).is_err());
    assert!(validate_loopback_endpoint("http://127.0.0.1:9223", &["http"]).is_ok());
}
