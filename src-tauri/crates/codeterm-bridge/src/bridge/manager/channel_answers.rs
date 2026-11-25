use super::*;

pub(super) async fn claim(
    tab: &Arc<dyn BridgeTabPort>,
    message: &InboundMessage,
    client: &reqwest::Client,
    api_base: &str,
    token: &str,
) -> bool {
    let ReplyContext::Telegram { chat_id, .. } = &message.reply_ctx;
    let Some(data) = message.callback_data.clone() else {
        return false;
    };
    let tab = Arc::clone(tab);
    let chat_id = *chat_id;
    let message_id = message.message_id;
    let Some(result) =
        claim_off_runtime(move || tab.claim_channel_answer(&data, chat_id, message_id)).await
    else {
        return false;
    };
    if let Err(reason) = result {
        let ReplyContext::Telegram {
            chat_id, thread_id, ..
        } = &message.reply_ctx;
        let reason: String = reason.chars().take(1000).collect();
        let mut payload = serde_json::json!({
            "chat_id":chat_id,
            "text":format!("Could not answer notification: {reason}"),
        });
        if let Some(thread_id) = thread_id {
            payload["message_thread_id"] = (*thread_id).into();
        }
        if let Some(message_id) = message.message_id {
            payload["reply_to_message_id"] = message_id.into();
        }
        let sent = client
            .post(tg_url(api_base, token, "sendMessage"))
            .json(&payload)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await;
        let delivered = match sent {
            Ok(response) if response.status().is_success() => response
                .json::<serde_json::Value>()
                .await
                .is_ok_and(|body| body["ok"] == true),
            _ => false,
        };
        if !delivered {
            log::warn!(
                "[bridge] notification answer refusal could not be delivered to the channel"
            );
        }
    }
    true
}

async fn claim_off_runtime(
    claim: impl FnOnce() -> Option<Result<(), String>> + Send + 'static,
) -> Option<Result<(), String>> {
    match tokio::task::spawn_blocking(claim).await {
        Ok(result) => result,
        Err(error) => {
            log::error!("[bridge] notification answer task stopped: {error}");
            Some(Err("The notification answer could not be confirmed".into()))
        }
    }
}

#[cfg(test)]
mod tests;
