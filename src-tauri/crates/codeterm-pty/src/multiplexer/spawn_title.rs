use crate::multiplexer::types::TitleLevel;

/// The single decision seam for whether a freshly spawned tab's title should
/// override the `create_tab`-generated name. Returns `None` when neither a
/// real explicit title nor a real agent name was supplied — callers MUST then
/// leave the generated name alone; the raw provider id / agent_cmd is never
/// an acceptable title on any spawn path.
pub fn creation_title_override(
    explicit: Option<&str>,
    agent_name: Option<&str>,
) -> Option<(String, TitleLevel)> {
    explicit
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .or_else(|| agent_name.map(str::trim).filter(|n| !n.is_empty()))
        .map(|t| (t.to_string(), TitleLevel::User))
}
