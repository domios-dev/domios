use super::apply::{CommandResult, CommandRunner};

const QUERY_FORMAT: &str = "-f=${Status}\\t${Version}";
const QUERY_ENV: [(&str, &str); 1] = [("LC_ALL", "C")];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DebianPackageState {
    pub(super) installed_version: Option<String>,
    pub(super) status: Option<String>,
    pub(super) audit: CommandResult,
    pub(super) query: CommandResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DebianInstallOutcome {
    Verified,
    VerificationFailed,
    InstallFailed,
}

impl DebianPackageState {
    pub(super) fn install_outcome(
        &self,
        install_success: bool,
        target_version: &str,
    ) -> DebianInstallOutcome {
        if self.is_consistent(target_version) {
            DebianInstallOutcome::Verified
        } else if install_success {
            DebianInstallOutcome::VerificationFailed
        } else {
            DebianInstallOutcome::InstallFailed
        }
    }

    pub(super) fn is_consistent(&self, target_version: &str) -> bool {
        self.installed_version.as_deref() == Some(target_version)
            && self.status.as_deref() == Some("install ok installed")
            && self.audit.success
            && self.audit.stdout.trim().is_empty()
    }
}

pub(super) fn staged_package_name(runner: &dyn CommandRunner, deb: &str) -> Result<String, String> {
    let metadata = runner.run("dpkg-deb", &["-f", deb, "Package"]);
    if metadata.success {
        if let Some(name) = parse_package_name(&metadata.stdout) {
            return Ok(name.to_string());
        }
    }
    Err(format!("blocked: Debian package metadata could not be read; install not attempted. metadata_exit={:?} metadata_stdout={:?} metadata_stderr={:?} staged_path={}", metadata.code, metadata.stdout, metadata.stderr, deb))
}

fn parse_package_name(stdout: &str) -> Option<&str> {
    let name = stdout.trim();
    let mut bytes = name.bytes();
    let first = bytes.next()?;
    if name.len() >= 2
        && (first.is_ascii_lowercase() || first.is_ascii_digit())
        && bytes.all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.')
        })
    {
        Some(name)
    } else {
        None
    }
}

pub(super) fn inspect_package_state(
    runner: &dyn CommandRunner,
    package_name: &str,
) -> DebianPackageState {
    let query = runner.run_with_env(
        "dpkg-query",
        &["-W", QUERY_FORMAT, package_name],
        &QUERY_ENV,
    );
    let (status, installed_version) = if query.success {
        parse_query_output(&query.stdout)
            .map(|(status, version)| (Some(status), Some(version)))
            .unwrap_or((None, None))
    } else {
        (None, None)
    };

    DebianPackageState {
        installed_version,
        status,
        audit: runner.run("dpkg", &["--audit"]),
        query,
    }
}

fn parse_query_output(stdout: &str) -> Option<(String, String)> {
    let mut lines = stdout.lines();
    let line = lines.next()?;
    if lines.next().is_some() {
        return None;
    }

    let (status, version) = line.split_once('\t')?;
    if status.is_empty() || version.is_empty() || version.contains('\t') {
        return None;
    }

    Some((status.to_string(), version.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Debug, PartialEq, Eq)]
    struct Invocation {
        program: String,
        args: Vec<String>,
        env: Vec<(String, String)>,
    }

    struct FakeRunner {
        responses: HashMap<String, CommandResult>,
        calls: RefCell<Vec<Invocation>>,
    }

    impl FakeRunner {
        fn with_outputs(query: &str) -> Self {
            let mut responses = HashMap::new();
            responses.insert(format!("dpkg-query -W {QUERY_FORMAT} code-term"), ok(query));
            responses.insert("dpkg --audit".to_string(), ok(""));
            Self {
                responses,
                calls: RefCell::new(Vec::new()),
            }
        }

        fn invoke(&self, program: &str, args: &[&str], env: &[(&str, &str)]) -> CommandResult {
            let key = format!("{} {}", program, args.join(" "));
            self.calls.borrow_mut().push(Invocation {
                program: program.to_string(),
                args: args.iter().map(|arg| arg.to_string()).collect(),
                env: env
                    .iter()
                    .map(|(name, value)| (name.to_string(), value.to_string()))
                    .collect(),
            });
            self.responses.get(&key).cloned().unwrap_or_else(|| fail(1))
        }
    }

    impl CommandRunner for FakeRunner {
        fn exists(&self, _program: &str) -> bool {
            false
        }

        fn run(&self, program: &str, args: &[&str]) -> CommandResult {
            self.invoke(program, args, &[])
        }

        fn run_with_env(
            &self,
            program: &str,
            args: &[&str],
            env: &[(&str, &str)],
        ) -> CommandResult {
            self.invoke(program, args, env)
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

    fn fail(code: i32) -> CommandResult {
        CommandResult {
            success: false,
            code: Some(code),
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    #[test]
    fn package_metadata_accepts_only_one_valid_package_name() {
        for name in ["code-term", "renamed-package", "libexample1+build.2"] {
            assert_eq!(parse_package_name(&format!("{name}\n")), Some(name));
        }
        for invalid in [
            "",
            "a",
            "CodeTerm",
            "-package",
            "one\ntwo",
            "pkg:amd64",
            "package name",
        ] {
            assert_eq!(parse_package_name(invalid), None);
        }
    }

    #[test]
    fn install_outcome_uses_install_result_and_verified_state() {
        let clean = DebianPackageState {
            installed_version: Some("1.14.2".to_string()),
            status: Some("install ok installed".to_string()),
            query: ok("install ok installed\t1.14.2"),
            audit: ok(""),
        };
        for success in [false, true] {
            assert_eq!(
                clean.install_outcome(success, "1.14.2"),
                DebianInstallOutcome::Verified
            );
        }
        let mut cases = Vec::new();
        let mut missing = clean.clone();
        missing.installed_version = None;
        missing.status = None;
        missing.query = fail(1);
        cases.push(missing);
        let mut old = clean.clone();
        old.installed_version = Some("1.14.1".to_string());
        cases.push(old);
        for status in ["install ok half-configured", "install ok unpacked"] {
            let mut partial = clean.clone();
            partial.status = Some(status.to_string());
            cases.push(partial);
        }
        let mut failed_audit = clean.clone();
        failed_audit.audit = fail(2);
        cases.push(failed_audit);
        let mut dirty_audit = clean;
        dirty_audit.audit = ok("another-package is half-configured");
        cases.push(dirty_audit);
        for state in cases {
            assert_eq!(
                state.install_outcome(true, "1.14.2"),
                DebianInstallOutcome::VerificationFailed
            );
            assert_eq!(
                state.install_outcome(false, "1.14.2"),
                DebianInstallOutcome::InstallFailed
            );
        }
    }

    #[test]
    fn debian_verify_installed_target_is_success() {
        let runner = FakeRunner::with_outputs("install ok installed\t1.7.1");
        let state = inspect_package_state(&runner, "code-term");
        assert!(state.is_consistent("1.7.1"));
        assert_eq!(state.installed_version.as_deref(), Some("1.7.1"));
        assert_eq!(state.status.as_deref(), Some("install ok installed"));
    }

    #[test]
    fn debian_verify_half_configured_is_repair_required() {
        for status in ["install ok half-configured", "install ok unpacked"] {
            let runner = FakeRunner::with_outputs(&format!("{status}\t1.7.1"));
            let state = inspect_package_state(&runner, "code-term");
            assert!(!state.is_consistent("1.7.1"), "status={status}");
        }
    }

    #[test]
    fn debian_verify_wrong_version_is_repair_required() {
        let runner = FakeRunner::with_outputs("install ok installed\t1.6.1");
        let state = inspect_package_state(&runner, "code-term");
        assert!(!state.is_consistent("1.7.1"));
    }

    #[test]
    fn debian_verify_parses_under_c_locale() {
        let runner = FakeRunner::with_outputs("install ok installed\t1.7.1");
        let state = inspect_package_state(&runner, "code-term");
        assert_eq!(state.installed_version.as_deref(), Some("1.7.1"));

        let calls = runner.calls.borrow();
        assert_eq!(calls[0].program, "dpkg-query");
        assert_eq!(
            calls[0].args,
            vec!["-W", "-f=${Status}\\t${Version}", "code-term"]
        );
        assert_eq!(calls[0].env, vec![("LC_ALL".to_string(), "C".to_string())]);
    }
}
