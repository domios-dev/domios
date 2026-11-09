#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRestoreContext<T> {
    pub instructions: Option<String>,
    pub additional_context: Vec<T>,
}

#[derive(serde::Serialize, ts_rs::TS)]
pub struct RuntimeAdditionalContext {
    pub key: String,
    pub kind: RuntimeAdditionalContextKind,
    pub value: String,
}

#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeAdditionalContextKind {
    Application,
    User,
}
