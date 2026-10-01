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
    fn git_failure_is_named_non_interactive_error() {
        let mut env = TestEnvGuard::lock();
        let directory = tempfile::tempdir().expect("tempdir");
        let git = install_fake_git(directory.path(), "printf 'remote denied\\n' >&2; exit 2");
        env.set("CODETERM_TEST_GIT_BIN", git.as_os_str());

        let error = run_git(vec!["fetch".into()]).expect_err("fake git fails");
        assert!(error.contains("git_non_interactive"));
        assert!(error.contains("git ran non-interactively with credential prompts disabled"));
        assert!(error.contains("remote denied"));
    }
}
