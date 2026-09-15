use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LaneDiagnosticFinding {
    pub tool: String,
    pub severity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LaneDiagnosticsSummary {
    pub plan_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub status: String,
    pub incomplete: bool,
    pub worktree: String,
    pub tools: Vec<String>,
    pub error_count: usize,
    pub warning_count: usize,
    pub info_count: usize,
    pub findings: Vec<LaneDiagnosticFinding>,
}

impl LaneDiagnosticsSummary {
    pub fn from_findings(
        plan_id: String,
        task_id: String,
        attempt_id: String,
        worktree: String,
        tools: Vec<String>,
        mut findings: Vec<LaneDiagnosticFinding>,
    ) -> Self {
        let error_count = findings
            .iter()
            .filter(|finding| finding.severity == "error")
            .count();
        let warning_count = findings
            .iter()
            .filter(|finding| finding.severity == "warning")
            .count();
        let info_count = findings
            .iter()
            .filter(|finding| finding.severity == "info")
            .count();
        findings.truncate(20);
        Self {
            plan_id,
            task_id,
            attempt_id,
            status: "complete".into(),
            incomplete: false,
            worktree,
            tools,
            error_count,
            warning_count,
            info_count,
            findings,
        }
    }

    pub fn incomplete(
        plan_id: String,
        task_id: String,
        attempt_id: String,
        worktree: String,
        message: String,
    ) -> Self {
        Self {
            plan_id,
            task_id,
            attempt_id,
            status: "incomplete".into(),
            incomplete: true,
            worktree,
            tools: Vec::new(),
            error_count: 0,
            warning_count: 1,
            info_count: 0,
            findings: vec![LaneDiagnosticFinding {
                tool: "lane-diagnostics".into(),
                severity: "warning".into(),
                message,
                ..Default::default()
            }],
        }
    }

    pub fn skipped(
        plan_id: String,
        task_id: String,
        attempt_id: String,
        worktree: String,
        reason: String,
    ) -> Self {
        Self {
            plan_id,
            task_id,
            attempt_id,
            status: "skipped".into(),
            incomplete: false,
            worktree,
            tools: vec!["build-gateway".into()],
            error_count: 0,
            warning_count: 1,
            info_count: 0,
            findings: vec![LaneDiagnosticFinding {
                tool: "build-gateway".into(),
                severity: "warning".into(),
                message: reason,
                ..Default::default()
            }],
        }
    }

    pub fn compact_lines(&self, limit: usize) -> Vec<String> {
        let mut lines = vec![format!(
            "diagnostics: {} errors, {} warnings, {} info ({})",
            self.error_count, self.warning_count, self.info_count, self.status
        )];
        lines.extend(self.findings.iter().take(limit).map(|finding| {
            let location = match (finding.file.as_deref(), finding.line, finding.column) {
                (Some(file), Some(line), Some(column)) => format!("{file}:{line}:{column}"),
                (Some(file), Some(line), None) => format!("{file}:{line}"),
                (Some(file), None, _) => file.to_string(),
                _ => "<repo>".into(),
            };
            let code = finding
                .code
                .as_deref()
                .map(|value| format!(" [{value}]"))
                .unwrap_or_default();
            format!("diagnostic: {} {}{}: {}", finding.severity, location, code, finding.message)
        }));
        lines
    }
}
