use crate::args::plan::PlanTaskAction;
use crate::client::RpcClient;
use super::{print_plan_result, task_cmd};

pub(crate) fn handle_mesh_dispatch(
    client: &mut dyn RpcClient,
    json: bool,
    task: PlanTaskAction,
    peer: &str,
) {
    match task_cmd::run_dispatch(client, json, task, Some(peer)) {
        Ok(None) => {}
        Ok(Some(value)) => print_plan_result(json, &value),
        Err(error) if json => crate::cmd::print::exit_with_cli_error(true, &error.message),
        Err(error) => {
            crate::cmd::plan_output::print_plan_error(false, &error.message);
            std::process::exit(1);
        }
    }
}
