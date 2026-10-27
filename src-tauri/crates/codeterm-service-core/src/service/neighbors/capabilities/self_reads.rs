use super::*;

impl ServiceContext {
    pub async fn neighbor_rpc_discovery(
        &self,
        credential: &str,
        method: &str,
    ) -> Result<Option<serde_json::Value>, String> {
        if method != "system.capabilities" {
            return Ok(None);
        }
        let Some(caller) = self.authenticate_local_credential(credential)? else {
            return Ok(None);
        };
        let Some((_, permission)) = self
            .neighbor_capability_permission_for(&caller, false)
            .await?
        else {
            return Ok(None);
        };
        let methods: Vec<_> = [
            "system.identify",
            "system.capabilities",
            "tab.agent_pad",
            "tab.todo.list",
            "api.request",
            "agent.report",
            "mem.search",
            "mem.get",
            "mem.wake_up",
        ]
        .into_iter()
        .filter(|method| {
            rpc_capability(method).is_some_and(|capability| capability.require(&permission).is_ok())
        })
        .collect();
        Ok(Some(serde_json::json!({"methods":methods,"plugins":[]})))
    }
}
