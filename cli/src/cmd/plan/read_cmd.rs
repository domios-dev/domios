use serde_json::{json, Value};

use crate::client::RpcClient;

use super::call;
use super::caller_context::CallerContext;

fn plan_state_value(
    client: &mut dyn RpcClient,
    plan_id: Option<String>,
    all: bool,
    page: u32,
) -> Result<Value, String> {
    let mut params = json!({"all":all,"page":page});
    if let Some(plan_id) = plan_id {
        params["plan_id"] = json!(plan_id);
    }
    CallerContext::from_env().apply_caller_tab(&mut params);
    call(client, "plan.state", params)
}

pub fn state(
    client: &mut dyn RpcClient,
    json: bool,
    plan_id: Option<String>,
    all: bool,
    page: u32,
) -> Result<Option<Value>, String> {
    let mut val = plan_state_value(client, plan_id, all, page)?;
    super::strip_display_paths(&mut val);
    if json {
        return Ok(Some(val));
    }
    print!("{}", crate::cmd::plan_state::render_state(&val));
    Ok(None)
}

pub fn lanes(
    client: &mut dyn RpcClient,
    json: bool,
    plan_id: Option<String>,
    stale: bool,
    all: bool,
    page: u32,
) -> Result<Option<Value>, String> {
    let val = plan_state_value(client, plan_id, all, page)?;
    if json {
        return Ok(Some(val));
    }
    print!(
        "{}{}",
        crate::cmd::plan_lanes::render_lanes(&val, stale),
        crate::cmd::plan_state::render_detail_windows(&val)
    );
    Ok(None)
}
