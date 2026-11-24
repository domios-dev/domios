use super::*;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct CronReportPlan {
    pub(crate) target: Option<String>,
    pub(crate) target_refused: bool,
    pub(crate) name: String,
    pub(crate) retire_oneshot: bool,
}

impl ServiceContext {
    #[cfg(feature = "test-support")]
    pub fn prepare_runtime_cron_owner_for_test(
        &self,
        tab: &str,
        job: &str,
        run: &str,
    ) -> Result<(), String> {
        self.prepare_runtime_cron_owner(tab, job, run)
    }

    pub(crate) fn cron_report_plan(&self, cron: &CronState, job_id: &str) -> CronReportPlan {
        let store = cron.lock();
        let job = store.get(job_id);
        CronReportPlan {
            target: job
                .and_then(|job| job.report_target_tab_id.as_ref())
                .filter(|target| target.len() <= 128)
                .cloned(),
            target_refused: job
                .and_then(|job| job.report_target_tab_id.as_ref())
                .is_some_and(|target| target.len() > 128),
            name: job
                .map(|job| job.name.trim().chars().take(256).collect::<String>())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| job_id.into()),
            retire_oneshot: job.is_some_and(|job| {
                job.oneshot
                    && !crate::service::role_policy::resolve(job.role.as_deref())
                        .is_some_and(|policy| policy.retain_oneshot())
            }),
        }
    }

    pub(crate) fn deliver_cron_terminal_report(
        &self,
        plan: &CronReportPlan,
        end: CronRunEnd<'_>,
        deliver: impl FnOnce(&str, MessageEnvelope, &str) -> Result<(), String>,
    ) -> String {
        if plan.target_refused {
            self.log_system_notice(
                "Cron report target refused".into(),
                Some(
                    "Report target identity exceeds its metadata bound; no delivery was invoked"
                        .into(),
                ),
                Some("error"),
            );
            return "failed: report target identity exceeds its bound".into();
        }
        let Some(target) = &plan.target else {
            return "no report target".into();
        };
        let report = format!(
            "Cron run {} completed with status {:?}. Summary: {}",
            end.run_id, end.status, end.summary
        );
        let envelope =
            MessageEnvelope::codeterm_reminder(cron_end_context(&plan.name, &end.status));
        cron_report_delivery_label(deliver(target, envelope, &report))
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn prepare_runtime_cron_owner(
        &self,
        tab_id: &str,
        job_id: &str,
        run_id: &str,
    ) -> Result<(), String> {
        let cron = self
            .extensions
            .try_get::<CronState>()
            .ok_or("Runtime cron state unavailable")?;
        self.run_watcher
            .reserve_cron_slot(self, cron, job_id, run_id)?;
        let linkage = crate::service::spawn::RuntimeOwnerLinkage::CronRun {
            job_id: job_id.into(),
            run_id: run_id.into(),
        };
        let owner = match self.run_watcher.cron_binding(tab_id, job_id, run_id) {
            Some(owner) => owner,
            None => self.reserve_cron_launch(
                cron,
                &linkage,
                tab_id,
                &self.ensure_session_binding_nonce(tab_id),
                None,
            )?,
        };
        self.bind_cron_spawn(
            cron,
            tab_id,
            job_id,
            run_id,
            owner.nonce().map(str::to_string),
            None,
        )
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn reserve_cron_launch(
        &self,
        cron: &CronState,
        linkage: &crate::service::spawn::RuntimeOwnerLinkage,
        tab: &str,
        nonce: &str,
        pty: Option<&str>,
    ) -> Result<crate::agent::run_watcher::runtime_runs::CronRunAuthority, String> {
        if let crate::service::spawn::RuntimeOwnerLinkage::CronRun { job_id, run_id } = linkage {
            self.run_watcher
                .reserve_cron_slot(self, cron, job_id, run_id)?;
        }
        let owner = self
            .run_watcher
            .capture_cron_launch(self, linkage, tab, nonce, pty)?;
        self.reserve_captured_cron_launch(cron, &owner)?;
        Ok(owner)
    }

    pub(crate) fn reserve_captured_cron_launch(
        &self,
        cron: &CronState,
        owner: &crate::agent::run_watcher::runtime_runs::CronRunAuthority,
    ) -> Result<(), String> {
        let mux = self.mux.read();
        if !owner.matches(&mux) {
            return Err("Cron launch changed before reservation".into());
        }
        let store = cron.lock();
        if !store
            .prepare_bind_run_tab(owner.job_id(), owner.run_id(), owner.tab_id())
            .value
        {
            return Err("Cron run cannot admit this launch".into());
        }
        if !self.run_watcher.bind_cron(owner.clone()) {
            return Err("Another launch retains this cron tab".into());
        }
        Ok(())
    }

    pub(crate) fn admit_cron_handle(
        &self,
        cron: &CronState,
        handle: &crate::service::spawn::TabHandle,
        job: &str,
        run: &str,
    ) -> Result<crate::agent::run_watcher::runtime_runs::CronRunAuthority, String> {
        let owner = self
            .run_watcher
            .cron_binding(&handle.tab_id, job, run)
            .ok_or("Cron spawn has no original launch witness")?;
        if owner.pty_id() != handle.pty_id.as_deref() {
            return Err("Cron spawn PTY generation changed".into());
        }
        if let Err(error) = self.bind_cron_spawn(
            cron,
            owner.tab_id(),
            job,
            run,
            owner.nonce().map(str::to_string),
            owner.pty_id().map(str::to_string),
        ) {
            self.close_owned_cron_launch(&owner, TabCloseReason::Cancelled);
            return Err(error);
        }
        Ok(owner)
    }

    pub(crate) fn close_owned_cron_launch(
        &self,
        owner: &crate::agent::run_watcher::runtime_runs::CronRunAuthority,
        reason: TabCloseReason,
    ) {
        self.run_watcher.retire_cron(owner);
        match self.settle_owned_cron_cleanup(owner, reason) {
            Ok(()) => {
                let cancelled = self.try_ext::<crate::CronState>().is_some_and(|cron| {
                    cron.lock().run_is_cancelled(owner.job_id(), owner.run_id())
                });
                if cancelled {
                    self.run_watcher.forget_cancelled_cron_finalizer(owner);
                }
            }
            Err(error) => {
                self.run_watcher.retain_cron_cleanup(owner, None);
                log::error!(
                    "Cron launch cleanup retained: job={} run={}: {error}",
                    owner.job_id(),
                    owner.run_id()
                );
                self.log_system_notice(
                    "Cron launch cleanup requires recovery".into(),
                    Some(error),
                    Some("error"),
                );
            }
        }
    }

    pub(crate) fn settle_owned_cron_cleanup(
        &self,
        owner: &crate::agent::run_watcher::runtime_runs::CronRunAuthority,
        reason: TabCloseReason,
    ) -> Result<(), String> {
        let (canonical, lease) = self.run_watcher.claim_cron_cleanup(owner)?;
        let _lease = lease.lock();
        let owner = &canonical;
        if let Some(pad) = self.run_watcher.cron_pad_lease(owner) {
            self.revoke_cron_pad(&pad)?;
        }
        if let Some(pty) = self.run_watcher.pending_cron_cleanup(owner) {
            pty.teardown.wait_for_completion(
                std::time::Instant::now() + std::time::Duration::from_secs(5),
            )?;
        }
        let current = owner.matches(&self.mux.read());
        let mut session = if current {
            owner
                .pty_id()
                .and_then(|pty| self.pty.lock().get_session(pty))
        } else {
            None
        };
        if current {
            let outcome = self.close_tab_with_teardown_and_cleanup(
                owner.tab_id(),
                reason,
                |mux, id| {
                    if !owner.matches(mux) {
                        return Err("Cron launch changed before cleanup".into());
                    }
                    mux.reserve_tab_close(id)
                },
                || Ok(()),
                |mux, _| {
                    if !owner.matches(mux) {
                        return Err("Cron launch changed during cleanup".into());
                    }
                    Ok(())
                },
                crate::service::tab::TabCloseCleanup::KeepPad,
            );
            if let Err(error) = outcome {
                if owner.matches(&self.mux.read()) {
                    self.run_watcher.retain_cron_cleanup(owner, None);
                    return Err(error);
                }
                session = None;
            }
        }
        let orphan = {
            let mux = self.mux.read();
            if let Some(pty) = owner.pty_id() {
                let attached = mux
                    .tab_id_for_pty(pty)
                    .and_then(|tab| mux.get_tab(&tab))
                    .is_some_and(|tab| tab.pty_id.as_deref() == Some(pty));
                if attached {
                    None
                } else {
                    let mut manager = self.pty.lock();
                    match manager.get_session(pty) {
                        Some(session) => Some((session, manager.begin_kill(pty)?)),
                        None => None,
                    }
                }
            } else {
                None
            }
        };
        let pending = if let Some((session, reaper)) = orphan {
            if let Err(error) = reaper.wait_bounded(std::time::Duration::from_secs(5)) {
                self.run_watcher.retain_cron_cleanup(owner, Some(session));
                return Err(error);
            }
            Some(session)
        } else {
            session
        };
        if let Some(session) = pending {
            if let Err(error) = session
                .teardown
                .wait_for_completion(std::time::Instant::now() + std::time::Duration::from_secs(5))
            {
                self.run_watcher.retain_cron_cleanup(owner, Some(session));
                return Err(error);
            }
        }
        if self
            .run_watcher
            .cron_allocation_pending(owner.job_id(), owner.run_id())
        {
            return Err(
                "Original cron allocation still owns teardown; cleanup will resume on release"
                    .into(),
            );
        }
        self.run_watcher.complete_cron_cleanup(owner);
        if let Some(wake) = self.try_ext::<crate::CronWake>() {
            wake.notify_all();
        }
        Ok(())
    }

    pub(crate) fn bind_cron_spawn(
        &self,
        cron: &CronState,
        tab_id: &str,
        job_id: &str,
        run_id: &str,
        binding_nonce: Option<String>,
        pty_id: Option<String>,
    ) -> Result<(), String> {
        let authority = self
            .run_watcher
            .cron_binding(tab_id, job_id, run_id)
            .ok_or("Cron launch witness is absent")?;
        if authority.nonce() != binding_nonce.as_deref() || authority.pty_id() != pty_id.as_deref()
        {
            return Err("Cron admission lost its original launch witness".into());
        }
        {
            let mux = self.mux.read();
            if !authority.matches(&mux) {
                return Err("Cron launch changed before durable admission".into());
            }
            let _ = runtime_cron_owner_needs_registration(&cron.lock(), tab_id, job_id, run_id)?;
        }
        let admitted = commit(cron, |store| {
            Ok(store.prepare_bind_run_tab(job_id, run_id, tab_id))
        })?;
        if !admitted.value {
            return Err("Cron run ended before tab admission".into());
        }
        let mux = self.mux.read();
        if !authority.matches(&mux) {
            return Err("Cron launch generation changed before admission".into());
        }
        let mut store = cron.lock();
        if !store.register_active_tab(tab_id, job_id, run_id) {
            return Err("Cron run ended during tab admission".into());
        }
        if !self.run_watcher.bind_cron(authority) {
            store.unregister_tab(tab_id);
            return Err("Cron launch witness changed during admission".into());
        }
        Ok(())
    }
}
