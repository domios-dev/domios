use serde::{Deserialize, Serialize};

pub const PROBE_RUNTIME_MS: u64 = 60_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GithubProbe {
    pub repo: String,
    pub resource: GithubResource,
    pub id: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GithubResource {
    Run,
    Jobs,
    PrChecks,
}

impl GithubProbe {
    pub fn command(&self) -> Result<String, String> {
        let parts: Vec<_> = self.repo.split('/').collect();
        if self.repo.len() > 256
            || parts.len() != 2
            || parts.iter().any(|part| {
                part.is_empty()
                    || !part
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
            })
            || self.id == 0
        {
            return Err("GitHub requires owner/repo and a positive run or PR id".into());
        }
        Ok(match self.resource {
            GithubResource::Run => format!(
                "gh run view {} --repo {} --json status,conclusion,url",
                self.id, self.repo
            ),
            GithubResource::Jobs => format!(
                "gh run view {} --repo {} --json status,conclusion,jobs,url",
                self.id, self.repo
            ),
            GithubResource::PrChecks => format!(
                "gh pr view {} --repo {} --json statusCheckRollup,url",
                self.id, self.repo
            ),
        })
    }
}

#[cfg(test)]
mod tests;
