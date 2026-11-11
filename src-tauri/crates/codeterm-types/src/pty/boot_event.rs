use serde::Serialize;
use ts_rs::TS;

use super::{BootStage, ProvisionalLaunch};

#[derive(Debug, Serialize, TS)]
pub struct TabBootStageEvent<'a> {
    pub tab_id: &'a str,
    pub stage: BootStage,
    pub agent_booting: bool,
    pub session_bound: bool,
    pub reason: Option<&'a str>,
    pub provisional_launch: Option<&'a ProvisionalLaunch>,
    pub launch_generation: TabLaunchGeneration<'a>,
}

#[derive(Debug, Serialize, TS)]
pub struct TabLaunchGeneration<'a> {
    pub epoch: Option<u64>,
    pub operation_id: Option<&'a str>,
    pub pty_id: Option<&'a str>,
    pub binding_nonce: Option<&'a str>,
}
