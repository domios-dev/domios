use crate::source::{self, RunBindOutcome, RunBinding, TabBinding};
use crate::storage::Staged;
use crate::store::{Sentinel, Store};

impl Store {
    pub fn stage_bind_run(
        &mut self,
        id: &str,
        cron_id: &str,
        run_id: &str,
        tab_id: &str,
    ) -> Staged<RunBindOutcome> {
        if let Err(error) = self.ensure_available() {
            return Staged::ready(Err(error));
        }
        if !source::bounded_id(cron_id)
            || !source::bounded_id(run_id)
            || !source::bounded_id(tab_id)
        {
            return Staged::ready(Err("invalid cron run binding identity".into()));
        }
        let Some(current) = self.get(id).cloned() else {
            return Staged::ready(Ok(RunBindOutcome::default()));
        };
        if current.machine.stopped
            || current
                .expires_at
                .is_some_and(|at| self.clock.now_ms() >= at)
            || current.observe().cloned() != Some(TabBinding::CronRun(cron_id.to_string()))
        {
            return Staged::ready(Ok(RunBindOutcome::default()));
        }
        if current
            .run
            .as_ref()
            .is_some_and(|bound| bound.run_id == run_id && bound.tab_id == tab_id)
        {
            return Staged::ready(Ok(RunBindOutcome::default()));
        }
        let Some(revision) = current.revision.checked_add(1) else {
            return Staged::ready(Err("blueprint run generation exhausted".into()));
        };
        let now = self.clock.now_ms();
        let replaced = current.run.clone();
        let reason = replaced.as_ref().map(|_| "run_replaced".to_string());
        let run = RunBinding {
            run_id: run_id.to_string(),
            tab_id: tab_id.to_string(),
            bound_at: now,
        };
        if !self.backend.is_json() {
            return self.sqlite_set_run_binding(
                id,
                current.revision,
                cron_id,
                replaced,
                Some(run),
                now,
                reason,
            );
        }
        let Some(position) = self.jobs.iter().position(|job| job.id == id) else {
            return Staged::ready(Ok(RunBindOutcome::default()));
        };
        let previous_wake = self.wake_pending.remove(id);
        let previous = self.jobs[position].clone();
        reset_for_binding(&mut self.jobs[position], Some(run), revision, now, reason);
        if let Err(error) = self.save_legacy() {
            self.jobs[position] = previous;
            if previous_wake {
                self.wake_pending.insert(id.to_string());
            }
            return Staged::ready(Err(error));
        }
        self.wake_pending.insert(id.to_string());
        if let Some(cancelled) = self.active.get(id) {
            cancelled.cancel();
        }
        Staged::ready(Ok(RunBindOutcome {
            changed: true,
            replaced,
        }))
    }

    pub fn stage_unbind_run(
        &mut self,
        id: &str,
        expected: &RunBinding,
        reason: &str,
    ) -> Staged<RunBindOutcome> {
        if let Err(error) = self.ensure_available() {
            return Staged::ready(Err(error));
        }
        let Some(current) = self.get(id).cloned() else {
            return Staged::ready(Ok(RunBindOutcome::default()));
        };
        let Some(bound) = current
            .run
            .as_ref()
            .filter(|bound| same_run(bound, expected))
        else {
            return Staged::ready(Ok(RunBindOutcome::default()));
        };
        let Some(revision) = current.revision.checked_add(1) else {
            return Staged::ready(Err("blueprint run generation exhausted".into()));
        };
        let cron_id = match current.observe() {
            Some(TabBinding::CronRun(cron_id)) => cron_id.clone(),
            _ => return Staged::ready(Ok(RunBindOutcome::default())),
        };
        let bound = bound.clone();
        let now = self.clock.now_ms();
        if !self.backend.is_json() {
            let staged = self.sqlite_set_run_binding(
                id,
                current.revision,
                &cron_id,
                Some(bound),
                None,
                now,
                Some(reason.to_string()),
            );
            return staged;
        }
        let Some(position) = self.jobs.iter().position(|job| job.id == id) else {
            return Staged::ready(Ok(RunBindOutcome::default()));
        };
        let previous_wake = self.wake_pending.remove(id);
        let previous = self.jobs[position].clone();
        reset_for_binding(
            &mut self.jobs[position],
            None,
            revision,
            now,
            Some(reason.to_string()),
        );
        if let Err(error) = self.save_legacy() {
            self.jobs[position] = previous;
            if previous_wake {
                self.wake_pending.insert(id.to_string());
            }
            return Staged::ready(Err(error));
        }
        if let Some(cancelled) = self.active.get(id) {
            cancelled.cancel();
        }
        Staged::ready(Ok(RunBindOutcome {
            changed: true,
            replaced: None,
        }))
    }
}

fn same_run(left: &RunBinding, right: &RunBinding) -> bool {
    left.run_id == right.run_id && left.tab_id == right.tab_id
}

fn reset_for_binding(
    job: &mut Sentinel,
    run: Option<RunBinding>,
    revision: u64,
    now: u64,
    reason: Option<String>,
) {
    job.run = run;
    job.run_transitions = 0;
    job.revision = revision;
    job.next_run = now;
    job.machine.last_hash = None;
    job.machine.last_bucket = None;
    job.progress = crate::progress::Progress::default();
    job.machine.held = None;
    job.last_reason = reason;
}
