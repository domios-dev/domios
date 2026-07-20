mod calibration;
mod graph;
pub(crate) mod item;
mod skill;
mod socket;
mod tail;
pub(crate) mod transport;
#[cfg(test)]
mod transport_audit;
mod worker;

use crate::client::SocketClient;
use crate::cmd::print::handle_result;
use crate::MemAction;

pub fn handle_mem(client: &mut SocketClient, json_mode: bool, action: MemAction) {
    if let Some(result) = socket::try_dispatch(client, &action) {
        print_mem_result(json_mode, result);
        return;
    }

    // Every remaining MemAction has full daemon socket parity except
    // `Secret` (deliberately kept off the unauthenticated socket — see
    // cmd::mem_secret) and `RunWorker`/`Dream` (short-circuited in main()
    // before a socket connection is even opened). History/Taxonomy/Tail/
    // Skill/Graph/Worker/Calibration keep their own CLI-side pretty-print
    // or local-only handling instead of the generic try_dispatch path, but
    // all now talk to `client`, never HTTP.
    match action {
        MemAction::History { id } => {
            let result = match item::handle_mem_history(client, id, json_mode) {
                Ok(None) => return,
                Ok(Some(v)) => Ok(v),
                Err(e) => Err(e),
            };
            handle_result(json_mode, result);
        }
        MemAction::Taxonomy { workspace } => {
            let result = match item::handle_mem_taxonomy(client, workspace, json_mode) {
                Ok(None) => return,
                Ok(Some(v)) => Ok(v),
                Err(e) => Err(e),
            };
            handle_result(json_mode, result);
        }
        MemAction::Tail { follow, limit } => {
            tail::run_mem_tail(client, json_mode, follow, limit);
        }
        MemAction::Skill { action } => {
            print_mem_result(json_mode, skill::handle_mem_skill(client, action));
        }
        MemAction::Graph { action } => {
            print_mem_result(json_mode, graph::handle_mem_graph(client, action));
        }
        MemAction::Worker { action } => {
            print_mem_result(json_mode, worker::handle_mem_worker(client, action));
        }
        MemAction::Calibration { action } => {
            print_mem_result(
                json_mode,
                calibration::handle_mem_calibration(client, action),
            );
        }
        MemAction::Secret { action } => {
            crate::cmd::mem_secret::handle_mem_secret(json_mode, action);
        }
        MemAction::RunWorker { .. } => unreachable!("RunWorker is short-circuited in main()"),
        MemAction::Dream { .. } => unreachable!("Dream is short-circuited in main()"),
        _ => unreachable!("all other mem actions have socket transport parity"),
    }
}

/// `{"text": "..."}` results (e.g. `mem wake-up`) print as raw text instead
/// of the generic `key: value` object dump — this is direct-injection
/// context, not structured data.
fn print_mem_result(json_mode: bool, result: Result<serde_json::Value, String>) {
    if let Ok(ref val) = result {
        if let Some(text) = val.get("text").and_then(|v: &serde_json::Value| v.as_str()) {
            println!("{}", text);
            return;
        }
    }
    handle_result(json_mode, result);
}

#[cfg(test)]
mod tests;
