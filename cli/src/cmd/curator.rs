use crate::args::curator::CuratorAction;
use crate::client::RpcClient;
use serde_json::{json, Value};

pub fn handle(client: &mut dyn RpcClient, action: CuratorAction) -> Result<Value, String> {
    match action {
        CuratorAction::Finish { next, from_pad } => {
            let note = super::payload_source::read_from_pad(&from_pad)?;
            let seconds = super::agent::parse_duration_secs(&next, "--next")?;
            let mut params = json!({"note":note,"next_secs":seconds});
            super::plan::inject_trusted_session(&mut params);
            super::plan::call_plan_rpc(client, "plan.curator.finish", params)
        }
    }
}
