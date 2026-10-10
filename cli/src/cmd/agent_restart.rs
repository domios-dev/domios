#[derive(serde::Serialize)]
pub(crate) struct RestartRequest<'a> {
    pub force: bool,
    pub grace_secs: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<&'a str>,
    #[serde(skip_serializing_if = "is_false")]
    pub handoff_history: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub fresh: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}
