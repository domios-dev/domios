pub fn resolve_spawn_workspace_id(
    requested_workspace_id: Option<String>,
    parent_tab_id: Option<&str>,
    caller_tab_id: Option<&str>,
    workspace_of_tab: impl Fn(&str) -> Option<String>,
    active_workspace_id: Option<String>,
    fallback_workspace_id: Option<String>,
) -> Result<String, String> {
    requested_workspace_id
        .or_else(|| parent_tab_id.and_then(&workspace_of_tab))
        .or_else(|| caller_tab_id.and_then(&workspace_of_tab))
        .or(active_workspace_id)
        .or(fallback_workspace_id)
        .ok_or_else(|| "No workspace found".to_string())
}

pub fn first_live_tab_id<'a>(
    spawner: Option<&'a str>,
    creator: Option<&'a str>,
) -> Option<&'a str> {
    [spawner, creator]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|id| !id.is_empty())
}

#[cfg(test)]
mod tests;
