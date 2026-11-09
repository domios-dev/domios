use super::catalog::BuildError;
use super::key::KeyPresence;
use super::{EmbedderIdentity, EmbedderStatus, ProviderDescriptor, ReembedProgress, SwitchPlan};
use codeterm_wire::api_contracts::Refusal;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct ProviderListing {
    #[serde(flatten)]
    pub descriptor: ProviderDescriptor,
    pub active: bool,
    pub key: Option<KeyPresence>,
    pub refusal: Option<Refusal>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct EmbedderListing {
    pub providers: Vec<ProviderListing>,
    pub remote_opt_in: bool,
    pub remote_warning: &'static str,
    pub status: EmbedderStatus,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct SwitchPreview {
    pub already_active: bool,
    pub already_running: Option<ReembedProgress>,
    pub cancelled: Option<EmbedderIdentity>,
    pub target: EmbedderIdentity,
    pub remote: bool,
    pub warning: Option<&'static str>,
    pub plan: SwitchPlan,
    pub total: u64,
    pub refusal: Option<Refusal>,
}

impl From<&BuildError> for Refusal {
    fn from(error: &BuildError) -> Self {
        Self {
            message: error.message(),
            hints: error.hints(),
        }
    }
}
