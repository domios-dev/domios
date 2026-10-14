use serde::Deserialize;

#[derive(Deserialize)]
pub struct SyncParams {
    pub path: Option<String>,
}
