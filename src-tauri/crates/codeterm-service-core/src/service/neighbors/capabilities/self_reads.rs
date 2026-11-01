use super::*;

impl ServiceContext {
    pub async fn neighbor_rpc_discovery(
        &self,
        credential: &str,
        method: &str,
        params: Option<&serde_json::Value>,
    ) -> Result<Option<serde_json::Value>, String> {
        if !matches!(method, "system.capabilities" | "tab.list") {
            return Ok(None);
        }
        let Some(caller) = self.authenticate_local_credential(credential)? else {
            return Ok(None);
        };
        let Some((_, permission)) = self
            .neighbor_capability_permission_for(&caller, method == "tab.list")
            .await?
        else {
            return Ok(None);
        };
        if method == "tab.list" {
            Capability::TabList.require(&permission)?;
            let mut listing = crate::dispatch::dispatch_tab_summary_if_ready(
                &crate::dispatch::Command::TabSummary {
                    workspace_id: params
                        .and_then(|params| params["workspace_id"].as_str())
                        .map(str::to_owned),
                    caller_tab_id: Some(caller.tab_id.clone()),
                    focused_workspace: params
                        .is_some_and(|params| params["focused_workspace"] == true),
                    agents_only: true,
                },
                self,
            )?
            .into_json();
            self.filter_neighbor_tab_listing(&caller, &mut listing)
                .await?;
            return Ok(Some(listing));
        }
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
            "mem.neighbor_save",
            "tab.list",
            "tab.send_text",
        ]
        .into_iter()
        .filter(|method| {
            rpc_capability(method).is_some_and(|capability| capability.require(&permission).is_ok())
        })
        .collect();
        Ok(Some(serde_json::json!({"methods":methods,"plugins":[]})))
    }
}
