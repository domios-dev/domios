use super::JsProvider;
use crate::provider::ReplyRelayTurn;
use serde_json::json;

pub(super) fn read(
    provider: &JsProvider,
    cwd: &str,
    session: &str,
    after: u64,
) -> Option<ReplyRelayTurn> {
    if !provider.has_reply_relay {
        return None;
    }
    provider
        .try_call::<Option<ReplyRelayTurn>>(
            "readReplyRelayTurn",
            &[json!(cwd), json!(session), json!(after)],
        )
        .flatten()
}
