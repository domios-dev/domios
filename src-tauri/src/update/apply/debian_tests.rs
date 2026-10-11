use super::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

struct FakeRunner {
    exists: HashSet<String>,
    results: HashMap<String, CommandResult>,
    calls: RefCell<Vec<String>>,
}

impl FakeRunner {
    fn new(programs: &[&str]) -> Self {
        Self {
            exists: programs.iter().map(|s| s.to_string()).collect(),
            results: HashMap::new(),
            calls: RefCell::new(Vec::new()),
        }
    }

    fn with_result(mut self, key: &str, result: CommandResult) -> Self {
        self.results.insert(key.to_string(), result);
        self
    }

    fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }
}

impl CommandRunner for FakeRunner {
    fn exists(&self, program: &str) -> bool {
        self.exists.contains(program)
    }

    fn run(&self, program: &str, args: &[&str]) -> CommandResult {
        let key = format!("{} {}", program, args.join(" "));
        self.calls.borrow_mut().push(key.clone());
        if program == "dpkg-deb" {
            return self
                .results
                .get(&key)
                .cloned()
                .unwrap_or_else(|| ok("code-term"));
        }
        self.results.get(&key).cloned().unwrap_or(CommandResult {
            success: true,
            code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
        })
    }

    fn run_with_env(&self, program: &str, args: &[&str], _env: &[(&str, &str)]) -> CommandResult {
        self.run(program, args)
    }
}

fn ok(stdout: &str) -> CommandResult {
    CommandResult {
        success: true,
        code: Some(0),
        stdout: stdout.to_string(),
        stderr: String::new(),
    }
}

fn fail(code: i32, stderr: &str) -> CommandResult {
    CommandResult {
        success: false,
        code: Some(code),
        stdout: String::new(),
        stderr: stderr.to_string(),
    }
}

fn clean_state(runner: FakeRunner, version: &str) -> FakeRunner {
    runner
        .with_result(
            "dpkg-query -W -f=${Status}\\t${Version} code-term",
            ok(&format!("install ok installed\t{version}")),
        )
        .with_result("dpkg --audit", ok(""))
}

#[test]
fn staged_metadata_selects_package_for_post_install_query() {
    let runner = FakeRunner::new(&["sudo"])
        .with_result("dpkg-deb -f /tmp/a.deb Package", ok("renamed-package\n"))
        .with_result(
            "dpkg-query -W -f=${Status}\\t${Version} renamed-package",
            ok("install ok installed\t1.14.2"),
        );
    install_verified_deb_with(std::path::Path::new("/tmp/a.deb"), "1.14.2", &runner, false)
        .unwrap();
    let calls = runner.calls();
    assert!(calls.contains(&"dpkg-query -W -f=${Status}\\t${Version} renamed-package".to_string()));
    assert!(!calls
        .iter()
        .any(|call| call.ends_with(" code-term") || call.ends_with(" codeterm")));
}

#[test]
fn invalid_or_unreadable_metadata_never_attempts_install() {
    for metadata in [ok(""), ok("one\ntwo"), fail(2, "invalid archive")] {
        let runner =
            FakeRunner::new(&["sudo"]).with_result("dpkg-deb -f /tmp/a.deb Package", metadata);
        let err =
            install_verified_deb_with(std::path::Path::new("/tmp/a.deb"), "1.14.2", &runner, false)
                .unwrap_err();
        assert!(err.starts_with("blocked:"));
        assert!(!runner.calls().iter().any(|call| call.contains("dpkg -i")));
    }
}

#[test]
fn passwordless_sudo_installs_without_password_prompt() {
    let runner = clean_state(FakeRunner::new(&["sudo"]), "1.7.1");
    let path = std::path::Path::new("/tmp/CodeTerm_1.7.1_amd64.deb");
    install_verified_deb_with(path, "1.7.1", &runner, false).unwrap();
    let calls = runner.calls();
    assert!(calls.contains(&"sudo -n true".to_string()));
    assert!(calls.contains(&"sudo -n dpkg -i /tmp/CodeTerm_1.7.1_amd64.deb".to_string()));
}

#[test]
fn password_required_sudo_returns_elevation_required() {
    let runner =
        FakeRunner::new(&["sudo"]).with_result("sudo -n true", fail(1, "a password is required"));
    let err =
        install_verified_deb_with(std::path::Path::new("/tmp/a.deb"), "1.7.1", &runner, false)
            .unwrap_err();
    assert!(err.contains("elevation_required"));
}

#[test]
fn headless_preflight_returns_elevation_required() {
    let runner =
        FakeRunner::new(&["sudo"]).with_result("sudo -n true", fail(1, "a password is required"));
    let preflight = debian_preflight_status_with_session(&runner, false);
    assert_eq!(preflight.status, DebianElevationStatus::ElevationRequired);
}

#[test]
fn gui_preflight_blocks_without_pkexec() {
    let runner = FakeRunner::new(&[]);
    let preflight = debian_preflight_status_with_session(&runner, true);
    assert_eq!(preflight.status, DebianElevationStatus::Blocked);
}

#[tokio::test(flavor = "current_thread")]
async fn no_update_does_not_run_elevation_preflight() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    let called = Arc::new(AtomicBool::new(false));
    let flag = called.clone();
    let gate = gate_staged_deb_async(DebianUpdatePlan::NoUpdate, move || {
        flag.store(true, Ordering::SeqCst);
        DebianPreflight {
            status: DebianElevationStatus::ElevationRequired,
            reason: Some("should not be observed".to_string()),
        }
    })
    .await
    .unwrap();
    assert!(matches!(gate, DebianStageGate::NoUpdate));
    assert!(!called.load(Ordering::SeqCst));
}

#[tokio::test(flavor = "current_thread")]
async fn staged_runs_preflight_through_async_boundary() {
    let gate = gate_staged_deb_async(
        DebianUpdatePlan::Staged {
            version: "1.7.1".to_string(),
            path: std::path::PathBuf::from("/tmp/CodeTerm_1.7.1_amd64.deb"),
        },
        || DebianPreflight {
            status: DebianElevationStatus::Ready,
            reason: Some("passwordless sudo available".to_string()),
        },
    )
    .await
    .unwrap();
    assert!(matches!(gate, DebianStageGate::Ready { .. }));
}

#[tokio::test(flavor = "current_thread")]
async fn staged_not_ready_surfaces_structured_status() {
    let gate = gate_staged_deb_async(
        DebianUpdatePlan::Staged {
            version: "1.7.1".to_string(),
            path: std::path::PathBuf::from("/tmp/a.deb"),
        },
        || DebianPreflight {
            status: DebianElevationStatus::ElevationRequired,
            reason: Some("headless requires elevation".to_string()),
        },
    )
    .await
    .unwrap();
    match gate {
        DebianStageGate::NotReady(p) => {
            assert_eq!(p.status, DebianElevationStatus::ElevationRequired)
        }
        _ => panic!("expected NotReady"),
    }
}

#[test]
fn pkexec_cancelled_is_not_success() {
    let runner = FakeRunner::new(&["pkexec"]).with_result(
        "pkexec dpkg -i /tmp/a.deb",
        fail(126, "dismissed authentication dialog"),
    );
    let err = install_verified_deb_with(std::path::Path::new("/tmp/a.deb"), "1.7.1", &runner, true)
        .unwrap_err();
    assert!(err.contains("elevation_cancelled"));
}

#[test]
fn command_not_found_is_blocked_not_cancelled() {
    let runner = FakeRunner::new(&["pkexec"])
        .with_result("pkexec dpkg -i /tmp/a.deb", fail(127, "pkexec: not found"));
    let err = install_verified_deb_with(std::path::Path::new("/tmp/a.deb"), "1.7.1", &runner, true)
        .unwrap_err();
    assert!(err.contains("blocked"));
    assert!(!err.contains("elevation_cancelled"));
}

#[test]
fn half_configured_state_repairs_successfully() {
    let runner = clean_state(
        FakeRunner::new(&["sudo"])
            .with_result("sudo -n dpkg -i /tmp/a.deb", fail(2, "postinst failed"))
            .with_result("sudo -n dpkg --configure -a", ok(""))
            .with_result("sudo -n apt-get install -f -y", ok("")),
        "1.7.1",
    );
    install_verified_deb_with(std::path::Path::new("/tmp/a.deb"), "1.7.1", &runner, false).unwrap();
    let calls = runner.calls();
    assert!(calls.contains(&"sudo -n dpkg --configure -a".to_string()));
    assert!(calls.contains(&"sudo -n apt-get install -f -y".to_string()));
}

#[test]
fn repair_failure_without_prior_package_returns_repair_required() {
    let runner = FakeRunner::new(&["sudo"])
        .with_result("sudo -n dpkg -i /tmp/a.deb", fail(2, "postinst failed"))
        .with_result("sudo -n dpkg --configure -a", fail(1, "still broken"))
        .with_result("sudo -n apt-get install -f -y", fail(1, "no fix"))
        .with_result(
            "dpkg-query -W -f=${Status}\\t${Version} code-term",
            fail(1, "not installed"),
        )
        .with_result("dpkg --audit", ok("codeterm is half-configured"));
    let err =
        install_verified_deb_with(std::path::Path::new("/tmp/a.deb"), "1.7.1", &runner, false)
            .unwrap_err();
    assert!(err.contains("repair_required"));
    assert!(err.contains("staged_path=/tmp/a.deb"));
    assert!(err.contains("sudo dpkg --configure -a"));
}

#[test]
fn resulting_version_mismatch_returns_verification_failed() {
    let runner = clean_state(FakeRunner::new(&["sudo"]), "1.6.1");
    let err =
        install_verified_deb_with(std::path::Path::new("/tmp/a.deb"), "1.7.1", &runner, false)
            .unwrap_err();
    assert!(err.contains("verification_failed"));
    assert!(!err.contains("sudo dpkg -i"));
    assert!(err.contains("installed_version=1.6.1"));
}

#[test]
fn half_configured_package_is_verification_failed_after_successful_install() {
    let runner = FakeRunner::new(&["sudo"])
        .with_result("sudo -n dpkg -i /tmp/a.deb", ok(""))
        .with_result(
            "dpkg-query -W -f=${Status}\\t${Version} code-term",
            ok("install ok half-configured\t1.7.1"),
        )
        .with_result("dpkg --audit", ok("code-term is half-configured"));
    let err =
        install_verified_deb_with(std::path::Path::new("/tmp/a.deb"), "1.7.1", &runner, false)
            .unwrap_err();
    assert!(err.contains("verification_failed"));
    assert!(!err.contains("sudo dpkg -i"));
    assert!(err.contains("status=install ok half-configured"));
}

#[test]
fn failed_empty_audit_is_verification_failed() {
    let runner = FakeRunner::new(&["sudo"])
        .with_result(
            "dpkg-query -W -f=${Status}\\t${Version} code-term",
            ok("install ok installed\t1.7.1"),
        )
        .with_result("dpkg --audit", fail(2, "audit backend failed"));
    let err =
        install_verified_deb_with(std::path::Path::new("/tmp/a.deb"), "1.7.1", &runner, false)
            .unwrap_err();
    assert!(err.contains("verification_failed"));
    assert!(!err.contains("sudo dpkg -i"));
    assert!(err.contains("audit_success=false"));
    assert!(err.contains("audit_exit=Some(2)"));
    assert!(err.contains("audit backend failed"));
}

#[test]
fn dpkg_query_failure_is_verification_failed() {
    let runner = FakeRunner::new(&["sudo"])
        .with_result(
            "dpkg-query -W -f=${Status}\\t${Version} code-term",
            fail(1, "query failed"),
        )
        .with_result("dpkg --audit", ok(""));
    let err =
        install_verified_deb_with(std::path::Path::new("/tmp/a.deb"), "1.7.1", &runner, false)
            .unwrap_err();
    assert!(err.contains("verification_failed"));
    assert!(!err.contains("sudo dpkg -i"));
    assert!(err.contains("installed_version=<none>"));
    assert!(err.contains("status=<unknown>"));
}

#[tokio::test(flavor = "current_thread")]
async fn staging_runs_through_async_boundary() {
    let path = stage_deb_atomically_async(b"deb bytes".to_vec(), "9.9.9".to_string())
        .await
        .unwrap();
    assert!(path.ends_with("CodeTerm_9.9.9_amd64.deb"));
    let body = std::fs::read(&path).unwrap();
    assert_eq!(body, b"deb bytes");
    let _ = std::fs::remove_file(path);
}

#[tokio::test(flavor = "current_thread")]
async fn installer_runs_through_async_boundary() {
    struct ThreadSafeRunner;

    impl CommandRunner for ThreadSafeRunner {
        fn exists(&self, program: &str) -> bool {
            program == "sudo"
        }

        fn run(&self, program: &str, args: &[&str]) -> CommandResult {
            if program == "dpkg-deb" {
                return ok("code-term");
            }
            if program == "dpkg-query" {
                assert_eq!(args, &["-W", "-f=${Status}\\t${Version}", "code-term"]);
                return ok("install ok installed\t1.7.1");
            }
            if program == "dpkg" {
                return ok("");
            }
            assert_eq!(program, "sudo");
            assert!(args.contains(&"-n"));
            CommandResult {
                success: true,
                code: Some(0),
                stdout: String::new(),
                stderr: String::new(),
            }
        }

        fn run_with_env(
            &self,
            program: &str,
            args: &[&str],
            _env: &[(&str, &str)],
        ) -> CommandResult {
            self.run(program, args)
        }
    }

    install_verified_deb_with_async(
        std::path::PathBuf::from("/tmp/CodeTerm_1.7.1_amd64.deb"),
        "1.7.1".to_string(),
        ThreadSafeRunner,
        false,
    )
    .await
    .unwrap();
}

#[test]
fn appimage_is_not_debian() {
    assert_ne!(InstallMethod::AppImage, InstallMethod::Deb);
}

// Accept the versioned Debian asset shape because it is the artifact path published by the release pipeline.
#[test]
fn genuine_debian_release_asset_url_is_accepted() {
    let url = Url::parse(
        "https://github.com/rollacode/codeterm-releases/releases/download/v1.10.9/CodeTerm_1.10.9_amd64.deb",
    )
    .unwrap();
    assert!(is_debian_package_url(&url));
}

#[test]
fn appimage_or_tarball_url_is_still_refused() {
    let appimage = Url::parse(
        "https://github.com/rollacode/codeterm-releases/releases/download/v1.10.9/CodeTerm_1.10.9_amd64.AppImage",
    )
    .unwrap();
    assert!(!is_debian_package_url(&appimage));

    let wrapped = Url::parse(
        "https://github.com/rollacode/codeterm-releases/releases/download/v1.10.9/CodeTerm_1.10.9_amd64.deb.tar.gz",
    )
    .unwrap();
    assert!(!is_debian_package_url(&wrapped));
}
