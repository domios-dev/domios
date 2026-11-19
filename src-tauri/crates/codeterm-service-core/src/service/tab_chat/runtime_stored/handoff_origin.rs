use crate::agent::chat_types::ChatMsg;
use crate::service::provider_handoff::input::{HANDOFF_CONTEXT, MAX_INPUT_BYTES};
use crate::service::tab_chat::chatbackend_history::handoff_origin_path;
use codeterm_util::private_file;
use serde::{Deserialize, Serialize};
use std::{io, path::Path};

const MAX_RECORD_BYTES: usize = MAX_INPUT_BYTES * 6;

#[derive(Serialize, Deserialize)]
pub(crate) struct HandoffOrigin {
    item_id: String,
    body: String,
    provider: Option<String>,
    model: Option<String>,
}

impl HandoffOrigin {
    pub(crate) fn recorded(row: &ChatMsg) -> Option<Self> {
        if row.kind != "user"
            || row.inbound_kind.as_deref() != Some("system")
            || row.from_context.as_deref() != Some(HANDOFF_CONTEXT)
        {
            return None;
        }
        Some(Self {
            item_id: row.runtime_source_item_id.clone()?,
            body: row.content.clone(),
            provider: row.from_label.clone(),
            model: row.from_model.clone(),
        })
    }

    pub(crate) fn save(&self, provider: &str, session: &str) -> io::Result<()> {
        self.write(&handoff_origin_path(provider, session))
    }

    fn write(&self, path: &Path) -> io::Result<()> {
        let bytes = serde_json::to_vec(self)?;
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(io::Error::other(
                "Provider handoff record exceeds its size bound",
            ));
        }
        private_file::write_atomic(path, &bytes)
    }

    fn read(path: &Path) -> io::Result<Self> {
        let file = private_file::open_owner_only(path, MAX_RECORD_BYTES as u64)?;
        Ok(serde_json::from_reader(file)?)
    }

    fn apply(self, rows: &mut [ChatMsg]) {
        if let Some(row) = rows
            .iter_mut()
            .find(|row| row.kind == "user" && row.id == self.item_id)
        {
            row.content = self.body;
            row.inbound_kind = Some("system".into());
            row.from_context = Some(HANDOFF_CONTEXT.into());
            row.from_label = self.provider;
            row.from_model = self.model;
            row.segments = None;
            row.segments_version = None;
        }
    }
}

pub(super) fn restore(provider: &str, session: &str, rows: &mut [ChatMsg]) {
    match HandoffOrigin::read(&handoff_origin_path(provider, session)) {
        Ok(origin) => origin.apply(rows),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            log::warn!("[runtime-transcript] provider handoff could not be restored: {error}")
        }
    }
}

#[cfg(test)]
mod tests;
