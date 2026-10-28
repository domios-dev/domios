use super::*;

pub(super) fn secret_path(path: &str) -> bool {
    let path = path.split('?').next().unwrap_or(path);
    path == "/api/secrets" || path.starts_with("/api/secrets/")
}

impl ServiceContext {
    pub(crate) fn authorize_neighbor_secret_rpc(
        &self,
        caller: &TabCaller,
        method: &str,
        params: Option<&serde_json::Value>,
    ) -> Result<(), String> {
        if method.starts_with("mem.secret.")
            || method == "api.request"
                && params
                    .and_then(|params| params["path"].as_str())
                    .is_some_and(secret_path)
        {
            self.authorize_neighbor_secret_caller(caller)?;
        }
        Ok(())
    }

    pub(super) fn authorize_neighbor_secret_caller(
        &self,
        caller: &TabCaller,
    ) -> Result<(), String> {
        let inherited = self
            .mux
            .read()
            .get_tab(&caller.tab_id)
            .is_some_and(|tab| tab.neighbor_authority.is_some());
        let remembered = self
            .neighbor_cell()
            .concierge_accounts
            .lock()
            .contains_key(&caller.tab_id);
        if inherited || remembered {
            // Secrets are always outside the allowlist, even while grants/account state reload.
            return Capability::Denied.require(&Permission::default());
        }
        Ok(())
    }
}
