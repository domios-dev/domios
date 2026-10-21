use serde::{Deserialize, Serialize};

pub const PROBE_RUNTIME_MS: u64 = 60_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GithubProbe {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub repo: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default = "default_timeout", skip_serializing_if = "is_default_timeout")]
    pub timeout_ms: u64,
    pub resource: GithubResource,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub id: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GithubResource {
    Run,
    Jobs,
    PrChecks,
    Runners,
}

impl GithubProbe {
    pub fn command(&self) -> Result<String, String> {
        if !(1..=PROBE_RUNTIME_MS).contains(&self.timeout_ms) {
            return Err("GitHub timeout_ms must be 1..60000".into());
        }
        if self.resource == GithubResource::Runners {
            let scope = match (&self.org, self.repo.is_empty()) {
                (Some(org), true) if valid_component(org) => format!("orgs/{org}"),
                (None, false) if valid_repo(&self.repo) => format!("repos/{}", self.repo),
                _ => return Err("GitHub runners require exactly one repo or org scope".into()),
            };
            if self.id != 0 || self.label.as_ref().is_some_and(|label| label.is_empty() || label.len() > 256 || label.chars().any(char::is_control)) {
                return Err("GitHub runners require a bounded label and no run id".into());
            }
            return Ok(format!("gh api --paginate --slurp '{scope}/actions/runners?per_page=100'"));
        }
        if !valid_repo(&self.repo) || self.id == 0 || self.org.is_some() || self.label.is_some() {
            return Err("GitHub requires owner/repo and a positive run or PR id".into());
        }
        Ok(match self.resource {
            GithubResource::Run | GithubResource::Jobs => format!(
                "gh run view {} --repo {} --json status,conclusion,jobs,url",
                self.id, self.repo
            ),
            GithubResource::Runners => unreachable!(),
            GithubResource::PrChecks => format!(
                "gh pr view {} --repo {} --json statusCheckRollup,url",
                self.id, self.repo
            ),
        })
    }
}

fn default_timeout() -> u64 { PROBE_RUNTIME_MS }
fn is_default_timeout(value: &u64) -> bool { *value == PROBE_RUNTIME_MS }
fn is_zero(value: &u64) -> bool { *value == 0 }
fn valid_component(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && value.bytes().all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
}
fn valid_repo(value: &str) -> bool {
    let parts: Vec<_> = value.split('/').collect();
    parts.len() == 2 && parts.iter().all(|part| valid_component(part))
}

#[cfg(test)]
mod tests;
