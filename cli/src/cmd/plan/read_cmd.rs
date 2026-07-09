use serde_json::{json, Value};

use crate::client::SocketClient;

use super::{call, maybe_inject_caller_pane};

fn plan_state_value(client: &mut SocketClient, plan_id: Option<String>) -> Result<Value, String> {
    let mut params = json!({});
    if let Some(plan_id) = plan_id {
        params["plan_id"] = json!(plan_id);
    }
    maybe_inject_caller_pane(&mut params);
    call(client, "plan.state", params)
}

pub fn state(
    client: &mut SocketClient,
    json: bool,
    plan_id: Option<String>,
) -> Result<Option<Value>, String> {
    let val = plan_state_value(client, plan_id)?;
    if json {
        return Ok(Some(val));
    }
    print!("{}", crate::cmd::plan_state::render_state(&val));
    Ok(None)
}

pub fn lanes(
    client: &mut SocketClient,
    json: bool,
    plan_id: Option<String>,
    stale: bool,
) -> Result<Option<Value>, String> {
    let val = plan_state_value(client, plan_id)?;
    if json {
        return Ok(Some(val));
    }
    print!("{}", crate::cmd::plan_lanes::render_lanes(&val, stale));
    Ok(None)
}
