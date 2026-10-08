use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CloseRestoreTabState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paired_planner_tab_id: Option<String>,
    pub command: Option<String>,
    pub stopped: bool,
    pub record_id: Option<String>,
    pub exact_session: bool,
}

impl CloseRestoreTabState {
    pub fn claim_rerun(&mut self) -> Result<Option<String>, String> {
        if !self.stopped {
            return Err("Restored shell has already been started".into());
        }
        self.stopped = false;
        Ok(self.command.clone())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellPurpose {
    Build,
    Dev,
    Qa,
    #[default]
    Shell,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellActivity {
    #[default]
    Unknown,
    Running,
    Idle,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellCommandState {
    pub activity: ShellActivity,
    pub last_exit_code: Option<i32>,
}
