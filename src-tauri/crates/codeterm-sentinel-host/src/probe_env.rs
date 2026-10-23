use super::probe::environment::Environment;

pub(crate) fn command() -> Environment {
    #[cfg(test)]
    if let Some(environment) = COMMAND_ENVIRONMENT.with(|slot| slot.borrow().clone()) {
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
thread_local! {
    static COMMAND_ENVIRONMENT: std::cell::RefCell<Option<Environment>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn with_command_environment<T>(environment: Environment, run: impl FnOnce() -> T) -> T {
    struct Restore(Option<Environment>);
    impl Drop for Restore {
        fn drop(&mut self) {
            COMMAND_ENVIRONMENT.with(|slot| slot.replace(self.0.take()));
        }
    }
    let _restore = Restore(COMMAND_ENVIRONMENT.with(|slot| slot.replace(Some(environment))));
    run()
}
