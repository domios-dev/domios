use super::{RuntimeEvent, RuntimeMaps};
use crate::service::messenger::FramedDelivery;
use codeterm_plugin::agent_runtime::MAX_RUNTIME_BATCH;
use codeterm_types::agent::chat_types::ChatMsg;

impl RuntimeMaps {
    pub(in crate::service::agent_runtime) fn routed_input(
        &self,
        send: &str,
        thread: &str,
    ) -> Option<ChatMsg> {
        let (owner, row) = self.routed_inputs.get(send)?;
        (owner == thread).then(|| row.clone())
    }

    pub(in crate::service::agent_runtime) fn remember_routed_input(
        &mut self,
        send: &str,
        thread: &str,
        frame: &FramedDelivery,
    ) -> Result<(), String> {
        if !self.routed_inputs.contains_key(send) && self.routed_inputs.len() >= MAX_RUNTIME_BATCH {
            return Err("Runtime routed input capacity exhausted".into());
        }
        let row = frame
            .chat_message(send.into())
            .ok_or("Invalid runtime routed input")?;
        self.routed_inputs.insert(send.into(), (thread.into(), row));
        Ok(())
    }

    pub(in crate::service::agent_runtime) fn project_routed_item(
        &self,
        event: &mut RuntimeEvent,
        evidence: &std::collections::HashSet<(String, Option<String>)>,
    ) {
        let RuntimeEvent::ItemUpdated { thread_id, item } = event else {
            return;
        };
        if item.kind != "user" {
            return;
        }
        let Some(source) = evidence.iter().find_map(|(send, id)| {
            if id.as_deref() != Some(item.id.as_str()) {
                return None;
            }
            let (thread, row) = self.routed_inputs.get(send)?;
            (thread == thread_id).then_some(row)
        }) else {
            return;
        };
        let ChatMsg {
            content,
            segments,
            segments_version,
            from_label,
            from_tab,
            from_model,
            from_purpose,
            from_context,
            from_mesh,
            from_plugin,
            plugin_icon_svg,
            inbound_kind,
            ..
        } = source.clone();
        item.content = content;
        item.segments = segments;
        item.segments_version = segments_version;
        item.from_label = from_label;
        item.from_tab = from_tab;
        item.from_model = from_model;
        item.from_purpose = from_purpose;
        item.from_context = from_context;
        item.from_mesh = from_mesh;
        item.from_plugin = from_plugin;
        item.plugin_icon_svg = plugin_icon_svg;
        item.inbound_kind = inbound_kind;
    }
}
