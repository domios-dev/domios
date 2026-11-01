use super::*;

#[derive(Default)]
struct Model {
    fail: Option<&'static str>,
    recovery_failure: Option<&'static str>,
    events: Vec<&'static str>,
    current: bool,
    running: bool,
}

impl Model {
    fn step(&mut self, name: &'static str, code: ErrorCode) -> Result<(), Error> {
        self.events.push(name);
        if self.fail == Some(name) || self.recovery_failure == Some(name) {
            Err(Error::new(code, name))
        } else {
            Ok(())
        }
    }
}

impl InstallPort for Model {
    fn download(&mut self) -> Result<(), Error> {
        self.step("download", ErrorCode::DownloadFailed)
    }
    fn verify(&mut self) -> Result<(), Error> {
        self.step("verify", ErrorCode::ArchiveMismatch)
    }
    fn stage(&mut self) -> Result<(), Error> {
        self.step("stage", ErrorCode::StageFailed)
    }
    fn swap(&mut self) -> Result<(), Error> {
        self.step("swap", ErrorCode::SwapFailed)?;
        self.current = false;
        Ok(())
    }
    fn restart(&mut self) -> Result<(), Error> {
        self.running = false;
        self.step("restart", ErrorCode::RestartFailed)?;
        self.running = true;
        Ok(())
    }
    fn health(&mut self) -> Result<(), Error> {
        self.step("health", ErrorCode::HealthFailed)
    }
    fn rollback(&mut self) -> Result<(), Error> {
        self.step("rollback", ErrorCode::RollbackFailed)?;
        self.current = true;
        Ok(())
    }
    fn restart_previous(&mut self) -> Result<(), Error> {
        self.step("restart_previous", ErrorCode::RestartFailed)?;
        self.running = true;
        Ok(())
    }
    fn health_previous(&mut self) -> Result<(), Error> {
        self.step("health_previous", ErrorCode::HealthFailed)
    }
    fn commit(&mut self) -> Result<(), Error> {
        self.step("commit", ErrorCode::StageFailed)
    }
}

#[test]
fn headless_pre_activation_failures_leave_old_binary_running() {
    for step in ["download", "verify", "stage", "swap"] {
        let mut model = Model {
            current: true,
            running: true,
            fail: Some(step),
            ..Default::default()
        };
        assert!(install(&mut model).is_err(), "{step}");
        assert!(model.current && model.running, "{step}");
        assert!(!model.events.contains(&"restart"), "{step}");
    }
}

#[test]
fn headless_activation_failures_restore_and_verify_previous_bundle() {
    for step in ["restart", "health", "commit"] {
        let mut model = Model {
            current: true,
            running: true,
            fail: Some(step),
            ..Default::default()
        };
        let error = install(&mut model).unwrap_err();
        assert_ne!(error.code, ErrorCode::RollbackFailed);
        assert!(model.current && model.running, "{step}");
        assert!(model
            .events
            .ends_with(&["rollback", "restart_previous", "health_previous"]));
    }
}

#[test]
fn headless_success_commits_only_after_health() {
    let mut model = Model {
        current: true,
        running: true,
        ..Default::default()
    };
    install(&mut model).unwrap();
    assert_eq!(
        model.events,
        ["download", "verify", "stage", "swap", "restart", "health", "commit"]
    );
    assert!(!model.current && model.running);
}

#[test]
fn headless_recovery_failures_are_explicit() {
    for step in ["rollback", "restart_previous", "health_previous"] {
        let mut model = Model {
            fail: Some(step),
            ..Default::default()
        };
        assert!(restore(&mut model).is_err(), "{step}");
        assert_eq!(model.events.last(), Some(&step));
        let mut failed_activation = Model {
            fail: Some("health"),
            recovery_failure: Some(step),
            ..Default::default()
        };
        assert_eq!(
            install(&mut failed_activation).unwrap_err().code,
            ErrorCode::RollbackFailed
        );
    }
}
