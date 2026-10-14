use super::{call, CallerContext};
use crate::client::RpcClient;
use serde_json::{json, Value};

pub fn clean_lanes(
    client: &mut dyn RpcClient,
    json: bool,
    plan_id: Option<String>,
    confirm: bool,
    fetch: bool,
) -> Result<Option<Value>, String> {
    let mut params = json!({"confirm": confirm, "fetch": fetch});
    if let Some(id) = plan_id {
        params["plan_id"] = json!(id);
    }
    CallerContext::from_env().apply_trusted_session(&mut params);
    let val = call(client, "plan.lanes_clean", params)?;
    if json {
        return Ok(Some(val));
    }
    print!("{}", crate::cmd::plan_lanes::render_lane_cleanup(&val));
    Ok(None)
}

#[cfg(test)]
mod tests;
