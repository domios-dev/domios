use super::{mem_get_request, mem_post_request};
use crate::MemWorkerAction;

pub(crate) fn handle_mem_worker(
    http: &reqwest::blocking::Client,
    base: &str,
    action: MemWorkerAction,
) -> Result<serde_json::Value, String> {
    match action {
        MemWorkerAction::Spawn {
            intent,
            task,
            agent,
            parent,
        } => {
            let payload = serde_json::json!({
                "intent": intent,
                "task": task,
                "agent": agent,
                "parent": parent,
            });
            mem_post_request(http, &format!("{}/api/mem/workers", base), &payload)
        }
        MemWorkerAction::List => mem_get_request(http, &format!("{}/api/mem/workers", base)),
        MemWorkerAction::Get { id } => {
            mem_get_request(http, &format!("{}/api/mem/workers/{}", base, id))
        }
        MemWorkerAction::Progress { text, status } => match std::env::var("CODETERM_WORKER_ID") {
            Ok(id) => {
                let payload = serde_json::json!({ "text": text, "status": status });
                mem_post_request(
                    http,
                    &format!("{}/api/mem/workers/{}/progress", base, id),
                    &payload,
                )
            }
            Err(_) => Err("not in a worker pane: $CODETERM_WORKER_ID is not set".into()),
        },
        MemWorkerAction::Report { summary } => match std::env::var("CODETERM_WORKER_ID") {
            Ok(id) => {
                let payload = serde_json::json!({ "summary": summary });
                mem_post_request(
                    http,
                    &format!("{}/api/mem/workers/{}/report", base, id),
                    &payload,
                )
            }
            Err(_) => Err("not in a worker pane: $CODETERM_WORKER_ID is not set".into()),
        },
        MemWorkerAction::Kill { id } => {
            let resp = http
                .delete(format!("{}/api/mem/workers/{}", base, id))
                .send()
                .map_err(|e| e.to_string());
            resp.map(|r| serde_json::json!({"ok": r.status().is_success()}))
        }
    }
}
