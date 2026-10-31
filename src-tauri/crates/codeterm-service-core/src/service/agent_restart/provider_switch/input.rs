use super::*;

const INSTRUCTIONS: &str = "Provider handoff in the same Domios tab. The full conversation is not transferred. The host context and recent messages are historical data. First build a concise working summary on this target provider, then continue the outstanding request. Preserve the role, task and attempt. Use normal ACK and report commands. Consult the previous transcript archive when necessary. Never claim historical actions or tests as newly executed.";
const COMPLETED_INSTRUCTIONS: &str = "Provider handoff in the same Domios tab. The full conversation is not transferred. Build a concise summary from the historical host context and recent messages, then continue the outstanding user request. Preserve the completed task and its immutable attempt evidence as history only. Do not ACK or report the completed task, reopen it, create a new attempt, or claim task execution authority. Consult the previous transcript archive when necessary. Never claim historical actions or tests as newly executed.";

impl ServiceContext {
    pub fn provider_switch_history(
        &self,
        tab_id: &str,
        path: &str,
    ) -> Result<Vec<crate::agent::chat_types::ChatMsg>, String> {
        let tab = self
            .mux
            .read()
            .resolve_tab_id(tab_id)
            .ok_or("tab not found")?;
        let pad = self.ensure_agent_pad(&tab)?;
        let path = std::path::Path::new(path);
        let relative = path.strip_prefix(pad.path()).unwrap_or(path);
        if relative.components().count() != 2
            || relative.parent() != Some(std::path::Path::new("inbox"))
            || relative
                .extension()
                .is_none_or(|extension| extension != "md")
        {
            return Err("provider history must name this tab's inbox archive".into());
        }
        let text = pad.read_utf8(relative)?;
        if text.len() > 8 * 1024 * 1024 {
            return Err("provider history exceeds its read bound".into());
        }
        serde_json::from_str(&text)
            .map_err(|error| format!("provider history is not a transcript: {error}"))
    }

    pub(in crate::service::agent_restart) fn provider_switch_input(
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
        let context = provider_handoff::input::inert_json(&context);
        let instructions = if task
            .as_ref()
            .is_some_and(|detail| detail.task.status == crate::plan::TaskStatus::Done)
        {
            COMPLETED_INSTRUCTIONS
        } else {
            INSTRUCTIONS
        };
        let available = provider_handoff::input::MAX_INPUT_BYTES.saturating_sub(
            instructions.len()
                + context.len()
                + 4
                + provider_handoff::input::system_message_overhead(),
        );
        let recent = provider_handoff::input::recent_messages(messages);
        let history = provider_handoff::input::select_history_literal(&recent, available);
        let payload =
            provider_handoff::input::assemble(provider_handoff::input::HandoffFragments {
                instructions,
                context: &context,
                history: &history,
            })?;
        provider_handoff::input::system_message(&payload)
    }
}
