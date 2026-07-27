use crate::client::SocketClient;
use crate::MemWorkerAction;

pub(crate) fn handle_mem_worker(
    client: &mut SocketClient,
    action: MemWorkerAction,
) -> Result<serde_json::Value, String> {
    match action {
        MemWorkerAction::Spawn {
            intent,
            task,
            agent,
            parent,
        } => client.call(
            "mem.worker.spawn",
            Some(serde_json::json!({
                "intent": intent,
                "task": task,
                "agent": agent,
                "parent": parent,
            })),
        ),
        MemWorkerAction::List => client.call("mem.worker.list", None),
        MemWorkerAction::Get { id } => {
            client.call("mem.worker.get", Some(serde_json::json!({ "id": id })))
        }
        MemWorkerAction::Progress { text, status } => match std::env::var("CODETERM_WORKER_ID") {
            Ok(id) => client.call(
                "mem.worker.progress",
                Some(serde_json::json!({ "id": id, "text": text, "status": status })),
            ),
            Err(_) => Err("not in a worker tab: $CODETERM_WORKER_ID is not set".into()),
        },
        MemWorkerAction::Report { summary } => match std::env::var("CODETERM_WORKER_ID") {
            Ok(id) => client.call(
                "mem.worker.report",
                Some(serde_json::json!({ "id": id, "summary": summary })),
            ),
            Err(_) => Err("not in a worker tab: $CODETERM_WORKER_ID is not set".into()),
        },
        MemWorkerAction::Kill { id } => {
            client.call("mem.worker.kill", Some(serde_json::json!({ "id": id })))
        }
    }
}
