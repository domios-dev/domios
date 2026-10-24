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
    clean_lanes_as(
        client,
        json,
        plan_id,
        confirm,
        fetch,
        CallerContext::from_env(),
    )
}

fn clean_lanes_as(
    client: &mut dyn RpcClient,
    json: bool,
    plan_id: Option<String>,
    confirm: bool,
    fetch: bool,
    caller: CallerContext,
) -> Result<Option<Value>, String> {
    let mut params = json!({"confirm": confirm, "fetch": fetch});
    if let Some(id) = plan_id {
        params["plan_id"] = json!(id);
    }
    if !json {
        params["stream"] = json!(true);
    }
    caller.apply_trusted_session(&mut params);
    let val = call(client, "plan.lanes_clean", params)?;
    if json {
        return Ok(Some(val));
    }
    let id = val
        .get("job_id")
        .and_then(Value::as_str)
        .ok_or("missing lane cleanup job id")?;
    let mut cursor = 0;
    loop {
        let update = call(
            client,
            "plan.lanes_clean_next",
            json!({"job_id": id, "cursor": cursor}),
        )?;
        let events = update["events"]
            .as_array()
            .ok_or("missing lane cleanup progress events")?;
        for event in events {
            let rows = json!({"lanes": [event["lane"].clone()]});
            print!("{}", crate::cmd::plan_lanes::render_lane_cleanup(&rows));
        }
        use std::io::Write;
        std::io::stdout()
            .flush()
            .map_err(|error| error.to_string())?;
        cursor = update["cursor"]
            .as_u64()
            .ok_or("missing lane cleanup progress cursor")?;
        if update["done"] == true {
            break;
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests;
