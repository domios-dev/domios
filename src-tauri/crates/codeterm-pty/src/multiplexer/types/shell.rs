use serde::{Deserialize, Serialize};

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
