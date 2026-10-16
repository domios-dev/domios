use super::*;

pub struct FakeRegisteredLaunch(LaunchGeneration);

impl FakeRegisteredLaunch {
    pub fn register(
        svc: &ServiceContext,
        tab_id: &str,
        pty_id: &str,
        origin: crate::service::AgentLaunchOrigin,
    ) -> Result<Self, String> {
        let mut launch = Self(LaunchGeneration::begin(tab_id, pty_id.into()));
        launch
            .0
            .capture_binding_nonce(svc, None, None, origin, None, None)?;
        Ok(launch)
    }

    pub fn attempt(&self) -> crate::service::launch_readiness::LaunchAttempt {
        self.0.attempt().expect("registered fake launch")
    }
}

impl Drop for FakeRegisteredLaunch {
    fn drop(&mut self) {
        if let Some(attempt) = self.0.attempt() {
            crate::service::launch_readiness::retire_attempt(&attempt);
        }
        LAUNCH_REGISTRY.lock().finish(&self.0);
    }
}
