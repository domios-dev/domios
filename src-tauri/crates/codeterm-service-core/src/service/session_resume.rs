use super::ServiceContext;

impl ServiceContext {
    pub fn existing_session_resume_tab(&self, session_id: &str) -> Result<Option<String>, String> {
        let mux = self.mux.read();
        let Some(tab_id) = mux.tab_ids_with_session(session_id).into_iter().next() else {
            return Ok(None);
        };
        let tab = mux
            .get_tab(&tab_id)
            .ok_or("Bound session tab disappeared")?;
        if !super::tab_has_live_agent(tab)
            || tab.agent_booting
            || matches!(
                tab.boot_stage,
                Some(codeterm_types::pty::BootStage::SpawnFailed)
            )
        {
            return Err(format!(
                "Session {session_id} is bound to Tab {tab_id}, but its agent is not ready; recover it with: domios agent restart --tab {tab_id}"
            ));
        }
        Ok(Some(tab_id))
    }
}

#[cfg(test)]
mod tests;
