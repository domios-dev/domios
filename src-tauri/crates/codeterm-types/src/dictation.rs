use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const SAMPLE_RATE: u32 = 16_000;
pub const MAX_PCM_BYTES: usize = SAMPLE_RATE as usize;
pub const MAX_AUDIO_BYTES: usize = SAMPLE_RATE as usize * 2 * 300;
pub const MAX_UPDATE_BYTES: usize = 8192;

#[derive(Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DictationStart {
    pub session: String,
    pub tab_id: String,
}

#[derive(Debug, Deserialize, Serialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum DictationStartResult {
    Ready,
    Setup { reason: String },
    Unavailable { reason: String },
}

#[derive(Debug, Default, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DictationSegment {
    pub committed: String,
    pub provisional: String,
}

#[derive(Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DictationUpdate {
    pub session: String,
    pub sequence: u32,
    pub committed: String,
    pub provisional: String,
    #[serde(rename = "final")]
    pub is_final: bool,
}
