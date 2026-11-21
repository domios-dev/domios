use super::*;

impl RunWatcherRegistry {
    pub(crate) fn bind_cron(&self, owner: CronRunAuthority) -> bool {
        let mut bindings = self.cron_bindings.lock();
        if bindings
            .get(&owner.tab_id)
            .is_some_and(|prior| prior != &owner)
        {
            return false;
        }
        self.retain_cron_cleanup(&owner, None);
        bindings.insert(owner.tab_id.clone(), owner);
        true
    }

    pub(crate) fn cron_binding(&self, tab: &str, job: &str, run: &str) -> Option<CronRunAuthority> {
        self.cron_bindings
            .lock()
            .get(tab)
            .filter(|owner| owner.job_id == job && owner.run_id == run)
            .cloned()
    }

    pub(crate) fn retire_cron(&self, owner: &CronRunAuthority) {
        {
            let mut runs = self.runtime_runs.lock();
            let matches = owner.pty_id.is_none()
                && runs.get(&owner.tab_id).is_some_and(|run| {
                    run.nonce.as_str() == owner.binding_nonce.as_deref().unwrap_or_default()
                        && run.kind
                            == (AgentRunKind::Cron {
                                job_id: owner.job_id.clone(),
                                run_id: owner.run_id.clone(),
                            })
                });
            if matches {
                if let Some(run) = runs.remove(&owner.tab_id) {
                    let _ = run.cancelled.send(true);
                }
            }
        }
        self.runs.lock().retain(|_, run| {
            let matches = run.tab_id == owner.tab_id
                && Some(run.pty_id.as_str()) == owner.pty_id.as_deref()
                && run.binding_nonce == owner.binding_nonce
                && run.kind
                    == (AgentRunKind::Cron {
                        job_id: owner.job_id.clone(),
                        run_id: owner.run_id.clone(),
                    });
            if matches {
                run.completed
                    .store(true, std::sync::atomic::Ordering::Release);
            }
            !matches
        });
    }

    pub(crate) fn release_cron_binding(&self, owner: &CronRunAuthority) {
        let mut bindings = self.cron_bindings.lock();
        if bindings.get(&owner.tab_id) == Some(owner) {
            bindings.remove(&owner.tab_id);
        }
    }

    pub(crate) fn attach_cron_pad(
        &self,
        owner: &CronRunAuthority,
        pad: crate::service::agent_pad::CronPadLease,
    ) -> Result<(), String> {
        self.retain_cron_cleanup(owner, None);
        let mut entries = self.cron_cleanup.lock();
        let entry = entries
            .get_mut(&(owner.job_id.clone(), owner.run_id.clone()))
            .ok_or("Cron pad cleanup owner is absent")?;
        if entry.owner.tab_id != owner.tab_id || entry.owner.binding_nonce != owner.binding_nonce {
            return Err("Cron pad belongs to a different original generation".into());
        }
        entry.pad = Some(pad);
        Ok(())
    }

    pub(crate) fn cron_pad_lease(
        &self,
        owner: &CronRunAuthority,
    ) -> Option<crate::service::agent_pad::CronPadLease> {
        self.cron_cleanup
            .lock()
            .get(&(owner.job_id.clone(), owner.run_id.clone()))
            .filter(|entry| {
                entry.owner.tab_id == owner.tab_id
                    && entry.owner.binding_nonce == owner.binding_nonce
            })
            .and_then(|entry| entry.pad.clone())
    }

    pub(crate) fn provisional_cron_generation(
        &self,
        tab: &crate::multiplexer::types::Tab,
        linkage: &crate::service::spawn::RuntimeOwnerLinkage,
    ) -> Option<CronRunAuthority> {
        let crate::service::spawn::RuntimeOwnerLinkage::CronRun { job_id, run_id } = linkage else {
            return None;
        };
        let owner = CronRunAuthority {
            tab_id: tab.id.clone(),
            job_id: job_id.clone(),
            run_id: run_id.clone(),
            binding_nonce: tab.session_binding_nonce.clone(),
            pty_id: tab.pty_id.clone(),
        };
        self.retain_cron_cleanup(&owner, None);
        Some(owner)
    }

    pub(crate) fn fail_sealed_cron_launch(
        &self,
        svc: &ServiceContext,
        owner: &CronRunAuthority,
        reason: &str,
    ) -> Option<cron_end::CronEndHandle> {
        let failed_start_epoch = match engine::for_tab(owner.tab_id())
            .filter(|session| Some(session.context.lock().binding_nonce.as_str()) == owner.nonce())
        {
            Some(_) => Some(ended_unbound_start(svc, owner.tab_id(), owner.nonce()?)?),
            None => None,
        };
        cron_end::finish(
            svc,
            cron_end::EndedCronRun {
                tab_id: owner.tab_id(),
                job_id: owner.job_id(),
                run_id: owner.run_id(),
                success: false,
                summary: format!("Runtime startup failed: {reason}"),
                raw_output: "",
                session_id: None,
                cwd: None,
                binding_nonce: owner.nonce(),
                pty_id: owner.pty_id(),
                failed_start_epoch,
            },
        )
    }

    pub(crate) fn begin_cron_cancellation(
        &self,
        job: &str,
    ) -> Result<CronCancellation<'_>, String> {
        if !self.cron_cancelling.lock().insert(job.into()) {
            return Err("Cron cancellation is already settling".into());
        }
        Ok(CronCancellation {
            registry: self,
            job: job.into(),
        })
    }

    pub(crate) fn cron_cleanup_owners(&self, job: &str) -> Vec<CronRunAuthority> {
        self.cron_cleanup
            .lock()
            .values()
            .filter(|entry| entry.owner.job_id == job)
            .map(|entry| entry.owner.clone())
            .collect()
    }

    pub(crate) fn claim_cron_cleanup(
        &self,
        owner: &CronRunAuthority,
    ) -> Result<(CronRunAuthority, std::sync::Arc<parking_lot::Mutex<()>>), String> {
        self.retain_cron_cleanup(owner, None);
        let pending = self.cron_cleanup.lock();
        let entry = pending
            .get(&(owner.job_id.clone(), owner.run_id.clone()))
            .ok_or("Cron cleanup owner missing")?;
        if entry.owner.tab_id != owner.tab_id || entry.owner.binding_nonce != owner.binding_nonce {
            return Err("Cron cleanup belongs to another original generation".into());
        }
        Ok((entry.owner.clone(), entry.gate.clone()))
    }

    pub(crate) fn pending_cron_cleanup(
        &self,
        owner: &CronRunAuthority,
    ) -> Option<std::sync::Arc<crate::pty::manager::PtySession>> {
        self.cron_cleanup
            .lock()
            .get(&(owner.job_id.clone(), owner.run_id.clone()))
            .filter(|entry| entry.owner == *owner)
            .and_then(|entry| entry.pty.clone())
    }

    pub(crate) fn retain_cron_cleanup(
        &self,
        owner: &CronRunAuthority,
        pty: Option<std::sync::Arc<crate::pty::manager::PtySession>>,
    ) {
        let mut pending = self.cron_cleanup.lock();
        let entry = pending
            .entry((owner.job_id.clone(), owner.run_id.clone()))
            .or_insert_with(|| CronCleanup {
                owner: owner.clone(),
                pty: None,
                gate: std::sync::Arc::new(parking_lot::Mutex::new(())),
                pad: None,
            });
        if entry.owner.tab_id == owner.tab_id && entry.owner.binding_nonce == owner.binding_nonce {
            if entry.owner.pty_id.is_none() && owner.pty_id.is_some() {
                entry.owner = owner.clone();
            }
            if pty.is_some() {
                entry.pty = pty;
            }
        }
    }

    pub(crate) fn complete_cron_cleanup(&self, owner: &CronRunAuthority) {
        if self.cron_allocation_pending(owner.job_id(), owner.run_id()) {
            return;
        }
        let mut pending = self.cron_cleanup.lock();
        let key = (owner.job_id.clone(), owner.run_id.clone());
        if pending.get(&key).is_some_and(|entry| entry.owner == *owner) {
            pending.remove(&key);
        }
        drop(pending);
        self.release_cron_binding(owner);
    }

    pub(crate) fn resume_cron_cleanup_for_run(
        &self,
        svc: &ServiceContext,
        run: &str,
        caller: Option<&str>,
        nonce: Option<&str>,
        human: bool,
    ) -> bool {
        let owner = self
            .cron_cleanup
            .lock()
            .values()
            .find(|entry| {
                entry.owner.run_id == run
                    && (human
                        || caller == Some(entry.owner.tab_id.as_str())
                            && nonce == entry.owner.nonce())
            })
            .map(|entry| entry.owner.clone());
        let Some(owner) = owner else { return false };
        svc.close_owned_cron_launch(&owner, TabCloseReason::AgentRunEnded);
        true
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn register_cron_for_test(
        &self,
        svc: ServiceContext,
        pty_id: String,
        tab_id: String,
        job_id: String,
        run_id: String,
        timeout_secs: u64,
    ) -> Result<(), String> {
        let owner = self.bind_cron_for_test(&svc, pty_id, tab_id, job_id, run_id)?;
        self.register_cron(svc, &owner, timeout_secs, None)
    }

    /// Records the launch witness only; the run watcher registers lazily on first completion.
    #[cfg(any(test, feature = "test-support"))]
    pub fn bind_cron_witness_for_test(
        &self,
        svc: &ServiceContext,
        pty_id: String,
        tab_id: String,
        job_id: String,
        run_id: String,
    ) -> Result<(), String> {
        self.bind_cron_for_test(svc, pty_id, tab_id, job_id, run_id)
            .map(|_| ())
    }

    #[cfg(any(test, feature = "test-support"))]
    fn bind_cron_for_test(
        &self,
        svc: &ServiceContext,
        pty_id: String,
        tab_id: String,
        job_id: String,
        run_id: String,
    ) -> Result<CronRunAuthority, String> {
        let binding_nonce = svc
            .mux
            .read()
            .get_tab(&tab_id)
            .and_then(|tab| tab.session_binding_nonce.clone());
        let owner = CronRunAuthority {
            tab_id,
            job_id,
            run_id,
            binding_nonce,
            pty_id: Some(pty_id),
        };
        if !self.bind_cron(owner.clone()) {
            return Err("Cron test binding conflicted with an existing witness".into());
        }
        Ok(owner)
    }
}
