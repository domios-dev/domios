use crate::args::plan::{PlanControlLoopAction, PlanCuratorAction};
use crate::client::RpcClient;
use serde_json::{json, Value};

pub fn control_loop(
    client: &mut dyn RpcClient,
    action: PlanControlLoopAction,
) -> Result<Value, String> {
    let (method, mut params) = match action {
        PlanControlLoopAction::Run { plan_id } => {
            ("plan.control_loop.run", json!({"plan_id":plan_id}))
        }
        PlanControlLoopAction::Show { plan_id } => {
            ("plan.control_loop.show", json!({"plan_id":plan_id}))
        }
    };
    super::plan::inject_trusted_session(&mut params);
    super::plan::call_plan_rpc(client, method, params)
}

pub fn grant(client: &mut dyn RpcClient, action: PlanCuratorAction) -> Result<Value, String> {
    let PlanCuratorAction::Grant {
        plan_id,
        curator_tab,
        blueprint,
        plan_skill,
        save,
    } = action;
    let mut params = json!({"plan_id":plan_id,"curator_tab":curator_tab,"blueprints":blueprint,"plan_skills":plan_skill,"saves":save});
    super::plan::inject_trusted_session(&mut params);
    super::plan::call_plan_rpc(client, "plan.curator.grant", params)
}
