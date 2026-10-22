use codeterm_service_core::config::schema::AppConfig;

pub(super) fn apply_isolated_listener_policy(config: &mut AppConfig) {
    config.api.enabled = true;
    config.api.host = "127.0.0.1".to_string();
    config.api.mesh_tailnet_host = None;
    config.api.cors = false;
    config.api.cors_allow_origins.clear();
    config.mesh.enabled = false;
}

#[cfg(test)]
mod tests;
