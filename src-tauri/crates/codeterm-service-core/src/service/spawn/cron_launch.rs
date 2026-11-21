use super::ServiceContext;
use crate::agent::run_watcher::runtime_runs::CronRunAuthority;
use std::collections::HashMap;

impl ServiceContext {
    pub fn link_hidden_agent_pty(&self, tab: &str, cwd: Option<String>, env: HashMap<String, String>) -> Result<String, String> {
        self.link_hidden_agent_cron_pty(tab, cwd, env, None)
    }

    pub(super) fn link_hidden_agent_cron_pty(&self, tab: &str, cwd: Option<String>, env: HashMap<String, String>, original: Option<&CronRunAuthority>) -> Result<String, String> {
        self.link_hidden_agent_cron_pty_with_builder(tab, original, |gate| {
            crate::pty::manager::build_session(gate, None, cwd, 120, 40, env)
        })
    }

    pub(crate) fn link_hidden_agent_cron_pty_with_builder(&self, tab: &str, original: Option<&CronRunAuthority>, build: impl FnOnce(crate::pty::manager::PtySpawnGate) -> Result<crate::pty::manager::BuiltSession, String>) -> Result<String, String> {
        let claim = self.pty_ownership.acquire(tab)?;
        if original.is_some_and(|owner| !owner.matches(&self.mux.read())) { return Err("Cron launch changed before PTY construction".into()); }
        let gate = self.pty.lock().spawn_gate();
        let built = build(gate)?;
        let candidate = self.pty.lock().register_session(None, built).0;
        let candidate_session = self.pty.lock().get_session(&candidate).ok_or("Cron candidate PTY disappeared before admission")?;
        if let Err(error) = super::super::pty_resize::resize(&self.pty, &self.screen_state, &candidate, None) {
            self.kill_ptys(&[candidate]);
            if let Some(original) = original { self.run_watcher.retain_cron_cleanup(original, Some(candidate_session)); }
            return Err(error);
        }
        let mut attached = None;
        let mut refusal = None;
        let selection = if original.is_none() {
            self.link_pty_with_claim(&claim, tab, candidate.clone(), true)
        } else { self.link_pty_with_admission(&claim, tab, candidate.clone(), true, |_| {}, |mux, selected| {
            let Some(original) = original else { return Ok(()) };
            let outcome = (|| {
                if selected.pty_id() != candidate { return Err("Cron launch cannot adopt a prior PTY claim".to_string()); }
                let owner = original.attach_claimed_pty(mux, selected)?;
                let cron = self.try_ext::<crate::CronState>().ok_or("Cron state unavailable during PTY admission")?;
                let store = cron.lock();
                if !store.prepare_bind_run_tab(owner.job_id(), owner.run_id(), tab).value { return Err("Cron run ended before PTY admission".into()); }
                if !self.run_watcher.bind_cron(owner.clone()) { return Err("Cron PTY launch witness changed".into()); }
                attached = Some(owner);
                Ok(())
            })();
            if let Err(error) = &outcome { refusal = Some(error.clone()); }
            outcome
        }) };
        if !selection.linked {
            if let Some(original) = original { self.run_watcher.retain_cron_cleanup(original, Some(candidate_session)); }
            return Err(refusal.unwrap_or_else(|| format!("Tab {tab} closed before PTY link")));
        }
        let mux = self.mux.read();
        if attached.as_ref().is_some_and(|owner| !owner.matches(&mux)) {
            return Err("Cron launch changed after PTY admission".into());
        }
        self.agent_run_pty_map.lock().insert(tab.into(), selection.pty_id.clone());
        Ok(selection.pty_id)
    }
}
