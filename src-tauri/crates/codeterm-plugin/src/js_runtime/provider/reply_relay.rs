use super::JsProvider;
use crate::provider::ReplyRelayEvidence;
use serde_json::json;

pub(super) fn read(
    provider: &JsProvider,
    cwd: &str,
    session: &str,
    after: u64,
) -> Option<ReplyRelayEvidence> {
    if !provider.has_reply_relay {
        return None;
    }
    provider
        .try_call::<Option<ReplyRelayEvidence>>(
            "readReplyRelayTurn",
            &[json!(cwd), json!(session), json!(after)],
        )
        .flatten()
        .filter(ReplyRelayEvidence::is_valid)
}
