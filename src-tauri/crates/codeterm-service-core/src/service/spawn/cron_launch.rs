use super::ServiceContext;
use crate::agent::run_watcher::runtime_runs::CronRunAuthority;
use std::collections::HashMap;

pub(crate) struct CronAllocation {
    svc: ServiceContext,
    job: String,
    run: String,
    armed: bool,
}

impl CronAllocation {
    pub(crate) fn claim(svc: &ServiceContext, job: &str, run: &str) -> Result<Self, String> {
        svc.run_watcher.claim_cron_allocation(job, run)?;
        Ok(Self {
            svc: svc.clone(),
            job: job.into(),
            run: run.into(),
            armed: true,
        })
    }

    pub(crate) fn captured(mut self, owner: CronRunAuthority) -> ProvisionalCronLaunch {
        self.armed = false;
        ProvisionalCronLaunch::new(&self.svc, owner)
    }
}

impl Drop for CronAllocation {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        if let Some(cron) = self.svc.try_ext::<crate::CronState>() {
            if let Err(error) = self
                .svc
                .run_watcher
                .settle_unallocated_cron_claim(&self.svc, cron, &self.job, &self.run)
            {
                self.svc.log_system_notice(
                    "Cron allocation outcome retained".into(),
                    Some(error),
                    Some("error"),
                );
            }
        }
    }
}

pub(crate) struct ProvisionalCronLaunch {
    svc: ServiceContext,
    owner: CronRunAuthority,
    armed: bool,
}

impl ProvisionalCronLaunch {
    pub(crate) fn new(svc: &ServiceContext, owner: CronRunAuthority) -> Self {
        Self {
            svc: svc.clone(),
            owner,
            armed: true,
        }
    }
    pub(crate) fn owner(&self) -> &CronRunAuthority {
        &self.owner
    }
    pub(crate) fn prepare(&self) -> Result<(), String> {
        self.svc.run_watcher.journal_cron_owner(&self.owner)
    }
    pub(crate) fn pad(&self) -> Result<(), String> {
        self.svc.capture_cron_pad(&self.owner).map(|_| ())
    }
    pub(crate) fn transfer(mut self) -> CronRunAuthority {
        self.armed = false;
        self.svc
            .run_watcher
            .release_cron_allocation(&self.svc, &self.owner);
        self.owner.clone()
    }
}

impl Drop for ProvisionalCronLaunch {
    fn drop(&mut self) {
        if self.armed {
            self.svc
                .run_watcher
                .release_cron_allocation(&self.svc, &self.owner);
            self.svc.run_watcher.retire_cron(&self.owner);
            if let Some(handle) = self.svc.run_watcher.fail_sealed_cron_launch(
                &self.svc,
                &self.owner,
                "provisional launch refused before owner transfer",
            ) {
                if let Err(error) = handle.wait_for_terminal() {
                    self.svc.log_system_notice(
                        "Cron provisional terminal outcome retained".into(),
                        Some(error),
                        Some("error"),
                    );
                }
                self.svc.close_owned_cron_launch(
                    &self.owner,
                    crate::multiplexer::types::TabCloseReason::SpawnRollback,
                );
            } else if self.svc.try_ext::<crate::CronState>().is_some_and(|cron| {
                cron.lock()
                    .run_is_cancelled(self.owner.job_id(), self.owner.run_id())
            }) {
                self.svc.close_owned_cron_launch(
                    &self.owner,
                    crate::multiplexer::types::TabCloseReason::Cancelled,
                );
            }
        }
    }
}

impl ServiceContext {
    pub(crate) fn capture_allocated_cron_generation(
        &self,
        expected: &crate::multiplexer::types::Tab,
        linkage: &crate::service::spawn::RuntimeOwnerLinkage,
    ) -> Result<CronRunAuthority, String> {
        let (nonce, committed) =
            self.reserve_agent_launch_binding_nonce_if_current(expected, None, None)?;
        if nonce.is_none() {
            return Err("Cron allocation cannot adopt an authenticated generation".into());
        }
        self.run_watcher
            .provisional_cron_generation(&committed, linkage)
            .ok_or_else(|| "Cron allocation has no original run linkage".into())
    }

    pub fn link_hidden_agent_pty(
        &self,
        tab: &str,
        cwd: Option<String>,
        env: HashMap<String, String>,
    ) -> Result<String, String> {
        self.link_hidden_agent_cron_pty(tab, cwd, env, None)
    }

    pub(super) fn link_hidden_agent_cron_pty(
        &self,
        tab: &str,
        cwd: Option<String>,
        env: HashMap<String, String>,
        original: Option<&CronRunAuthority>,
    ) -> Result<String, String> {
        self.link_hidden_agent_cron_pty_with_builder(tab, original, |gate| {
            crate::pty::manager::build_session(gate, None, cwd, 120, 40, env)
        })
    }

    pub(crate) fn link_hidden_agent_cron_pty_with_builder(
        &self,
        tab: &str,
        original: Option<&CronRunAuthority>,
        build: impl FnOnce(
            crate::pty::manager::PtySpawnGate,
        ) -> Result<crate::pty::manager::BuiltSession, String>,
    ) -> Result<String, String> {
        let claim = self.pty_ownership.acquire(tab)?;
        if original.is_some_and(|owner| !owner.matches(&self.mux.read())) {
            return Err("Cron launch changed before PTY construction".into());
        }
        let gate = self.pty.lock().spawn_gate();
        let built = build(gate)?;
        let candidate = self.pty.lock().register_session(None, built).0;
        let candidate_session = self
            .pty
            .lock()
            .get_session(&candidate)
            .ok_or("Cron candidate PTY disappeared before admission")?;
        if let Err(error) =
            super::super::pty_resize::resize(&self.pty, &self.screen_state, &candidate, None)
        {
            self.kill_ptys(&[candidate]);
            if let Some(original) = original {
                self.run_watcher
                    .retain_cron_cleanup(original, Some(candidate_session));
            }
            return Err(error);
        }
        let mut attached = None;
        let mut refusal = None;
        let selection = if original.is_none() {
            self.link_pty_with_claim(&claim, tab, candidate.clone(), true)
        } else {
            self.link_pty_with_admission(
                &claim,
                tab,
                candidate.clone(),
                true,
                |_| {},
                |mux, selected| {
                    let Some(original) = original else {
                        return Ok(());
                    };
                    let outcome = (|| {
                        if selected.pty_id() != candidate {
                            return Err("Cron launch cannot adopt a prior PTY claim".to_string());
                        }
                        let owner = original.attach_claimed_pty(mux, selected)?;
                        let cron = self
                            .try_ext::<crate::CronState>()
                            .ok_or("Cron state unavailable during PTY admission")?;
                        let store = cron.lock();
                        if !store
                            .prepare_bind_run_tab(owner.job_id(), owner.run_id(), tab)
                            .value
                        {
                            return Err("Cron run ended before PTY admission".into());
                        }
                        if !self.run_watcher.bind_cron(owner.clone()) {
                            return Err("Cron PTY launch witness changed".into());
                        }
                        attached = Some(owner);
                        Ok(())
                    })();
                    if let Err(error) = &outcome {
                        refusal = Some(error.clone());
                    }
                    outcome
                },
            )
        };
        if !selection.linked {
            if let Some(original) = original {
                self.run_watcher
                    .retain_cron_cleanup(original, Some(candidate_session));
            }
            return Err(refusal.unwrap_or_else(|| format!("Tab {tab} closed before PTY link")));
        }
        let mux = self.mux.read();
        if attached.as_ref().is_some_and(|owner| !owner.matches(&mux)) {
            return Err("Cron launch changed after PTY admission".into());
        }
        self.agent_run_pty_map
            .lock()
            .insert(tab.into(), selection.pty_id.clone());
        drop(mux);
        drop(claim);
        if let Some(owner) = attached {
            self.run_watcher.journal_cron_owner(&owner)?;
        }
        Ok(selection.pty_id)
    }
}
