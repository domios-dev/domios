use super::apply::{CommandResult, CommandRunner};

const PACKAGE_NAME: &str = "code-term";
const QUERY_FORMAT: &str = "-f=${Status}\\t${Version}";
const QUERY_ENV: [(&str, &str); 1] = [("LC_ALL", "C")];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DebianPackageState {
    pub(super) installed_version: Option<String>,
    pub(super) status: Option<String>,
    pub(super) audit: CommandResult,
}

impl DebianPackageState {
    pub(super) fn is_consistent(&self, target_version: &str) -> bool {
        self.installed_version.as_deref() == Some(target_version)
            && self.status.as_deref() == Some("install ok installed")
            && self.audit.success
            && self.audit.stdout.trim().is_empty()
    }
}

pub(super) fn inspect_package_state(runner: &dyn CommandRunner) -> DebianPackageState {
    let query = runner.run_with_env(
        "dpkg-query",
        &["-W", QUERY_FORMAT, PACKAGE_NAME],
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
            responses.insert(
                format!("dpkg-query -W {QUERY_FORMAT} {PACKAGE_NAME}"),
                ok(query),
            );
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
    fn debian_verify_installed_target_is_success() {
        let runner = FakeRunner::with_outputs("install ok installed\t1.7.1");
        let state = inspect_package_state(&runner);
        assert!(state.is_consistent("1.7.1"));
        assert_eq!(state.installed_version.as_deref(), Some("1.7.1"));
        assert_eq!(state.status.as_deref(), Some("install ok installed"));
    }

    #[test]
    fn debian_verify_half_configured_is_repair_required() {
        for status in ["install ok half-configured", "install ok unpacked"] {
            let runner = FakeRunner::with_outputs(&format!("{status}\t1.7.1"));
            let state = inspect_package_state(&runner);
            assert!(!state.is_consistent("1.7.1"), "status={status}");
        }
    }

    #[test]
    fn debian_verify_wrong_version_is_repair_required() {
        let runner = FakeRunner::with_outputs("install ok installed\t1.6.1");
        let state = inspect_package_state(&runner);
        assert!(!state.is_consistent("1.7.1"));
    }

    #[test]
    fn debian_verify_parses_under_c_locale() {
        let runner = FakeRunner::with_outputs("install ok installed\t1.7.1");
        let state = inspect_package_state(&runner);
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
