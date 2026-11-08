use super::{RuntimeEvent, RuntimeMaps};
use crate::service::messenger::FramedDelivery;
use codeterm_plugin::agent_runtime::MAX_RUNTIME_BATCH;
use codeterm_types::agent::chat_types::ChatMsg;

impl RuntimeMaps {
    pub(in crate::service::agent_runtime) fn written_input_id(_thread: &str, send: &str) -> String {
        send.to_owned()
    }

    pub(in crate::service::agent_runtime) fn project_written_item(
        &self,
        event: &RuntimeEvent,
        evidence: &std::collections::HashSet<(String, Option<String>)>,
    ) -> Option<RuntimeEvent> {
        match event {
            RuntimeEvent::ItemUpdated { thread_id, item } if item.kind == "user" => {
                let source = self
                    .items
                    .iter()
                    .find(|stored| {
                        stored.thread_id == *thread_id
                            && stored.item.kind == "user"
                            && (stored.item.runtime_source_item_id.as_ref() == Some(&item.id)
                                || stored.item.id == item.id
                                    && stored.item.runtime_source_item_id.is_some())
                    })
                    .or_else(|| {
                        evidence.iter().find_map(|(send, id)| {
                            if id.as_deref() != Some(item.id.as_str()) {
                                return None;
                            }
                            let own = Self::written_input_id(thread_id, send);
                            self.items.iter().find(|stored| {
                                stored.thread_id == *thread_id
                                    && stored.item.id == own
                                    && stored.item.kind == "user"
                            })
                        })
                    })?;
                let mut projected = source.item.clone();
                projected.runtime_source_item_id = source
                    .item
                    .runtime_source_item_id
                    .clone()
                    .or(Some(item.id.clone()));
                Some(RuntimeEvent::ItemUpdated {
                    thread_id: thread_id.clone(),
                    item: projected,
                })
            }
            RuntimeEvent::EvidenceUpdated {
                thread_id,
                send_id,
                turn_id,
                item_id: Some(item_id),
            } => {
                let own = Self::written_input_id(thread_id, send_id);
                self.items.iter().find(|stored| {
                    stored.thread_id == *thread_id
                        && stored.item.id == own
                        && stored.item.runtime_source_item_id.as_ref() == Some(item_id)
                })?;
                Some(RuntimeEvent::EvidenceUpdated {
                    thread_id: thread_id.clone(),
                    send_id: send_id.clone(),
                    turn_id: turn_id.clone(),
                    item_id: Some(own),
                })
            }
            _ => None,
        }
    }

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

    /// The row a send shows in Chat when the wire text it travels in carries more than itself.
    pub(in crate::service::agent_runtime) fn remember_routed_row(
        &mut self,
        send: &str,
        thread: &str,
        row: ChatMsg,
    ) -> Result<(), String> {
        if !self.routed_inputs.contains_key(send) && self.routed_inputs.len() >= MAX_RUNTIME_BATCH {
            return Err("Runtime routed input capacity exhausted".into());
        }
        self.routed_inputs.insert(send.into(), (thread.into(), row));
        Ok(())
    }

    pub(in crate::service::agent_runtime) fn remember_digest(
        &mut self,
        head: &str,
        members: Vec<String>,
    ) {
        self.digest_members.insert(head.into(), members);
    }

    pub(in crate::service::agent_runtime) fn take_digest(&mut self, head: &str) -> Vec<String> {
        self.digest_members.remove(head).unwrap_or_default()
    }

    pub(in crate::service::agent_runtime) fn project_routed_response_item(
        &mut self,
        send: &str,
        thread: &str,
        item: &mut ChatMsg,
    ) {
        let own = Self::written_input_id(thread, send);
        if let Some(source) = self.items.iter().find(|stored| {
            stored.thread_id == thread && stored.item.id == own && stored.item.kind == "user"
        }) {
            let mut projected = source.item.clone();
            projected.runtime_source_item_id = projected
                .runtime_source_item_id
                .clone()
                .or(Some(item.id.clone()));
            *item = projected;
            super::upsert_item(self, thread, item.clone());
            return;
        }
        if let Some((owner, source)) = self.routed_inputs.get(send) {
            if owner == thread {
                copy_routed_body(item, source);
            }
        }
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
        copy_routed_body(item, source);
    }
}

fn copy_routed_body(item: &mut ChatMsg, source: &ChatMsg) {
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
