use super::*;

struct TestEnvGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    previous: Vec<(std::ffi::OsString, Option<std::ffi::OsString>)>,
}

impl TestEnvGuard {
    fn lock() -> Self {
        Self {
            _lock: crate::test_env_lock()
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
            previous: Vec::new(),
        }
    }

    fn set(&mut self, key: impl AsRef<std::ffi::OsStr>, value: impl AsRef<std::ffi::OsStr>) {
        let key = key.as_ref().to_os_string();
        self.remember(&key);
        std::env::set_var(key, value);
    }

    fn remove(&mut self, key: impl AsRef<std::ffi::OsStr>) {
        let key = key.as_ref().to_os_string();
        self.remember(&key);
        std::env::remove_var(key);
    }

    fn remember(&mut self, key: &std::ffi::OsString) {
        if self.previous.iter().any(|(saved, _)| saved == key) {
            return;
        }
        self.previous.push((key.clone(), std::env::var_os(key)));
    }
}

impl Drop for TestEnvGuard {
    fn drop(&mut self) {
        for (key, value) in self.previous.drain(..).rev() {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

#[cfg(unix)]
fn install_fake_git(directory: &std::path::Path, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = directory.join("fake-git.sh");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write fake git");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .expect("chmod fake git");
    path
}

#[cfg(unix)]
mod git_never_prompts {
    use super::*;

    #[test]
    fn shutdown_fences_discovery_to_real_operation_admission() {
        use std::os::unix::ffi::OsStrExt;
        let _env = TestEnvGuard::lock();
        let directory = tempfile::tempdir().unwrap();
        let fifo = directory.path().join("gate");
        let name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let marker = directory.path().join("operation-spawned");
        let git = install_fake_git(directory.path(), &format!(
            "if [ \"$1\" = '--version' ]; then printf 'git version 2.21.0\\n'; exec /bin/cat '{}'; else : > '{}'; fi",
            fifo.display(), marker.display()
        ));
        let (started, ready) = std::sync::mpsc::channel();
        let epoch = crate::git_probe::epoch();
        let worker = std::thread::spawn(move || {
            run_git_with(vec!["clone".into()], None, epoch, || {
                let _ = crate::git_probe::capture_observed(
                    &git,
                    std::time::Instant::now() + std::time::Duration::from_secs(5),
                    epoch,
                    |pid| started.send(pid).unwrap(),
                );
                Ok(Command::new(git))
            })
        });
        let pid = ready
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        assert_eq!(stop_live_git_children(), 1);
        assert_eq!(
            worker.join().unwrap().unwrap_err(),
            "plugin channel git operation cancelled"
        );
        assert!(!marker.exists());
        assert!(LIVE_GIT_CHILDREN.lock().unwrap().is_empty());
        let mut status = 0;
        assert_eq!(
            unsafe { libc::waitpid(pid as libc::pid_t, &mut status, libc::WNOHANG) },
            -1
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
    }

    #[test]
    fn git_command_sets_non_interactive_env() {
        let mut env = TestEnvGuard::lock();
        let directory = tempfile::tempdir().expect("tempdir");
        let capture = directory.path().join("capture.txt");
        let git = install_fake_git(
            directory.path(),
            "{ printf '%s\\n' \"$GIT_TERMINAL_PROMPT\"; printf '%s\\n' \"$GCM_INTERACTIVE\"; if read -r line; then printf 'stdin-data\\n'; else printf 'stdin-eof\\n'; fi; printf '%s\\n' \"$GIT_SSH_COMMAND\"; } > \"$CODETERM_TEST_GIT_CAPTURE\"",
        );
        env.set("CODETERM_TEST_GIT_BIN", git.as_os_str());
        env.set("CODETERM_TEST_GIT_CAPTURE", capture.as_os_str());
        env.remove("GIT_SSH_COMMAND");

        run_git(vec!["probe".into()]).expect("fake git succeeds");

        assert_eq!(
            std::fs::read_to_string(capture).expect("capture"),
            "0\nnever\nstdin-eof\nssh -o BatchMode=yes\n"
        );
    }

    #[test]
    fn shutdown_stops_a_git_child_that_is_still_running() {
        let mut env = TestEnvGuard::lock();
        let directory = tempfile::tempdir().expect("tempdir");
        let started = directory.path().join("started");
        let git = install_fake_git(
            directory.path(),
            ": > \"$CODETERM_TEST_GIT_STARTED\"; sleep 30",
        );
        env.set("CODETERM_TEST_GIT_BIN", git.as_os_str());
        env.set("CODETERM_TEST_GIT_STARTED", started.as_os_str());

        let runner = std::thread::spawn(|| run_git(vec!["clone".into()]));
        while !started.exists() {
            std::thread::yield_now();
        }

        assert_eq!(stop_live_git_children(), 1);
        let result = runner.join().expect("runner thread");
        assert!(result.unwrap_err().starts_with("git failed"));
        assert_eq!(stop_live_git_children(), 0);
    }

    #[test]
    fn git_access_denial_names_its_cause() {
        let mut env = TestEnvGuard::lock();
        let directory = tempfile::tempdir().expect("tempdir");
        let git = install_fake_git(
            directory.path(),
            "printf 'remote: Repository not found.\\n' >&2; exit 128",
        );
        env.set("CODETERM_TEST_GIT_BIN", git.as_os_str());

        let error = run_git(vec!["fetch".into()]).expect_err("fake git fails");
        assert_eq!(error, crate::channel_failure::REPOSITORY_NOT_FOUND);
    }
}
