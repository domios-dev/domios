use serde::Deserialize;

#[derive(Deserialize)]
pub struct ExtractPendingParams {
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Deserialize, Default)]
pub struct DreamParams {
    #[serde(default)]
    pub phase: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct DreamDiaryParams {
    #[serde(default)]
    pub offset: Option<usize>,
    #[serde(default)]
    pub limit: Option<usize>,
}
