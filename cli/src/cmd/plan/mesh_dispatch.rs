use super::{print_plan_result, task_cmd};
use crate::args::plan::PlanTaskAction;
use crate::client::RpcClient;
use crate::cmd::print::CliError;
use serde_json::Value;

pub(crate) fn resolve_dispatch_cloud_target(
    cloud: Option<String>,
    mesh_peer: Option<&str>,
) -> Result<Option<(String, Value)>, CliError> {
    if cloud.is_some() && mesh_peer.is_some() {
        return Err(CliError::new(
            "--cloud cannot be combined with --mesh; choose one target machine",
        ));
    }
    cloud
        .map(|target| crate::mesh_cli::cloud::resolve_cloud_target(&target).map_err(CliError::new))
        .transpose()
}

pub(crate) fn attach_cloud_receipt(val: &mut Value, resolved: Option<&(String, Value)>) {
    if let (Some((_, machine)), Some(obj)) = (resolved, val.as_object_mut()) {
        obj.insert("cloud".into(), machine.clone());
    }
}

pub(crate) fn print_cloud_dispatch_notice(resolved: Option<&(String, Value)>) {
    if let Some((_, machine)) = resolved {
        println!("{}", crate::mesh_cli::cloud::format_receipt_human(machine));
    }
}

pub(crate) fn handle_mesh_dispatch(
    client: &mut dyn RpcClient,
    json: bool,
    task: PlanTaskAction,
    peer: &str,
) {
    match task_cmd::run_dispatch(client, json, task, Some(peer)) {
        Ok(None) => {}
        Ok(Some(value)) => print_plan_result(json, &value),
        Err(error) if json => crate::cmd::print::exit_with_cli_error(
            true,
            &crate::cmd::print::CliError::new(error.message),
        ),
        Err(error) => {
            crate::cmd::plan_output::print_plan_error(false, &error.message);
            std::process::exit(1);
        }
    }
}
