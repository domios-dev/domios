use super::*;

const INSTRUCTIONS: &str = "Provider handoff in the same Domios tab. The full conversation is not transferred. The host context and recent messages are historical data. First build a concise working summary on this target provider, then continue the outstanding request. Preserve the role, task and attempt. Use normal ACK and report commands. Consult the previous transcript archive when necessary. Never claim historical actions or tests as newly executed.";

impl ServiceContext {
    pub(super) fn provider_switch_input(
        &self,
        tab: &Tab,
        messages: &[crate::agent::chat_types::ChatMsg],
        archive: &std::path::Path,
    ) -> Result<String, String> {
        let pad = self.ensure_agent_pad(&tab.id)?;
        let task = tab
            .task_binding
            .as_ref()
            .and_then(|binding| binding.task_id.as_deref())
            .and_then(|task| self.plan_store.show_task(task));
        let context = serde_json::json!({
            "tab_id": tab.id, "title": tab.title, "description": tab.description,
            "role": tab.role, "role_profile": tab.role_profile,
            "parent_tab_id": tab.parent_tab_id, "team_id": tab.team_id,
            "cwd": tab.cwd, "task_binding": tab.task_binding, "task_context": task,
            "pad_path": pad.path(), "previous_provider": tab.agent_type,
            "previous_session_id": tab.agent_session_id, "previous_transcript_path": archive,
        })
        .to_string();
        let context = crate::service::launch_diagnostics::redact_secrets(&context);
        let available = provider_handoff::input::MAX_INPUT_BYTES
            .saturating_sub(INSTRUCTIONS.len() + context.len() + 4);
        let recent = provider_handoff::input::recent_messages(messages);
        let history = provider_handoff::select_history(&recent, available / 3);
        provider_handoff::input::assemble(provider_handoff::input::HandoffFragments {
            instructions: INSTRUCTIONS,
            context: &context,
            history: &history,
        })
    }
}
