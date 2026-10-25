use super::probe::environment::Environment;

pub(crate) fn command() -> Environment {
    #[cfg(test)]
    if let Some(environment) = COMMAND_ENVIRONMENT.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|fixture| fixture.environment.clone())
    }) {
        return environment;
    }
    let cli = codeterm_service_core::service::instance_env::instance_cli_binary_path();
    Environment::for_command(
        std::env::vars_os(),
        cli.as_deref(),
        if cfg!(target_os = "macos") {
            &["/opt/homebrew/bin", "/usr/local/bin"]
        } else {
            &[]
        },
    )
}

#[cfg(test)]
struct CommandFixture {
    environment: Environment,
    diagnostic: Option<String>,
}

#[cfg(test)]
thread_local! {
    static COMMAND_ENVIRONMENT: std::cell::RefCell<Option<CommandFixture>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn with_command_environment<T>(
    environment: Environment,
    run: impl FnOnce() -> T,
) -> (T, Option<String>) {
    struct Restore(Option<CommandFixture>);
    impl Drop for Restore {
        fn drop(&mut self) {
            COMMAND_ENVIRONMENT.with(|slot| slot.replace(self.0.take()));
        }
    }
    let _restore = Restore(COMMAND_ENVIRONMENT.with(|slot| {
        slot.replace(Some(CommandFixture {
            environment,
            diagnostic: None,
        }))
    }));
    let result = run();
    let diagnostic = COMMAND_ENVIRONMENT.with(|slot| {
        slot.borrow_mut()
            .as_mut()
            .and_then(|fixture| fixture.diagnostic.take())
    });
    (result, diagnostic)
}

#[cfg(test)]
pub(crate) fn record_output(
    result: &Result<crate::probe::Output, crate::probe::process::ProbeFailure>,
) {
    COMMAND_ENVIRONMENT.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(fixture) = slot.as_mut() else { return };
        let tail = crate::probe::test_receipt::stderr_tail;
        fixture.diagnostic = Some(match result {
            Ok(output) => format!(
                "exit={:?}; success={}; stdout={:?}; stderr={:?}",
                output.exit_code,
                output.success,
                tail(&output.stdout_text()),
                tail(&output.stderr_text())
            ),
            Err(failure) => format!(
                "exit={:?}; probe_reason={:?}; stdout=unavailable; stderr={:?}",
                failure.exit_code,
                failure.reason(),
                tail(&failure.stderr_tail)
            ),
        });
    });
}
