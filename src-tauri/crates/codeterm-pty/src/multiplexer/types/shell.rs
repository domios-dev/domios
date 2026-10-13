use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CloseRestoreTabState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utility_owner_tab_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paired_planner_tab_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_cwd: Option<String>,
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
    Deploy,
    Logs,
    Db,
    #[default]
    Shell,
}

impl ShellPurpose {
    pub const ALL: [Self; 7] = [
        Self::Build,
        Self::Dev,
        Self::Qa,
        Self::Deploy,
        Self::Logs,
        Self::Db,
        Self::Shell,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Dev => "dev",
            Self::Qa => "qa",
            Self::Deploy => "deploy",
            Self::Logs => "logs",
            Self::Db => "db",
            Self::Shell => "shell",
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utility_owner_survives_shell_restore() {
        let state = CloseRestoreTabState {
            utility_owner_tab_id: Some("owner".into()),
            command: Some("build".into()),
            stopped: true,
            ..Default::default()
        };
        let mut restored: CloseRestoreTabState =
            serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
        assert_eq!(restored.claim_rerun().unwrap().as_deref(), Some("build"));
        assert_eq!(restored.utility_owner_tab_id.as_deref(), Some("owner"));
    }
}
