use crate::args::effort::EffortAction;
use crate::client::RpcClient;
use serde_json::{json, Value};

fn call(client: &mut dyn RpcClient, method: &str, params: Value) -> Result<Value, String> {
    crate::cmd::plan::call_plan_rpc(client, method, params)
}

pub fn handle_effort(client: &mut dyn RpcClient, json_output: bool, action: EffortAction) {
    match run_effort(client, json_output, action) {
        Ok(None) => {}
        Ok(Some(val)) if json_output => {
            println!("{}", serde_json::to_string_pretty(&val).unwrap());
        }
        Ok(Some(val)) => crate::cmd::print::print_object(&val),
        Err(e) => {
            println!("{e}");
            std::process::exit(1);
        }
    }
}

fn run_effort(
    client: &mut dyn RpcClient,
    json_output: bool,
    action: EffortAction,
) -> Result<Option<Value>, String> {
    let result = match action {
        EffortAction::Show { plan_id } => {
            call(client, "effort.show", json!({ "plan_id": plan_id }))
        }
        EffortAction::Link { from, to, rel } => call(
            client,
            "effort.link",
            json!({ "from": from, "to": to, "rel": rel }),
        ),
    }?;
    if json_output {
        Ok(Some(result))
    } else {
        crate::cmd::print::print_object(&result);
        Ok(None)
    }
}
