use serde::Deserialize;
use serde_json::json;

use super::wire::SerdeSessionMessage;
use super::JsProvider;
use crate::provider::{SessionContent, SessionContentQuery, SessionContentSource, SessionMessage};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireSessionContent {
    #[serde(default)]
    messages: Vec<SerdeSessionMessage>,
    #[serde(default)]
    truncated: bool,
}

impl JsProvider {
    pub(super) fn js_session_content_source(&self) -> SessionContentSource {
        if self.has_js("readSessionContent") {
            return SessionContentSource::SharedStore;
        }
        if self.has_js("parseSessionMessages") {
            return SessionContentSource::SessionFile;
        }
        self.native()
            .map(|n| n.session_content_source())
            .unwrap_or(SessionContentSource::None)
    }

    pub(super) fn js_read_session_content(
        &self,
        session_id: &str,
        query: &SessionContentQuery,
    ) -> Option<SessionContent> {
        if !self.has_js("readSessionContent") {
            return self
                .native()
                .and_then(|n| n.read_session_content(session_id, query));
        }
        let wire = self
            .try_call::<Option<WireSessionContent>>(
                "readSessionContent",
                &[json!(session_id), json!(query)],
            )
            .flatten()?;
        Some(SessionContent {
            messages: wire
                .messages
                .into_iter()
                .map(SessionMessage::from)
                .collect(),
            truncated: wire.truncated,
        })
    }
}
