use std::collections::BTreeMap;

use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CompatibilityManifest {
    #[serde(default)]
    pub supported: Vec<String>,
    #[serde(default)]
    pub breaking: Vec<BreakingRange>,
    #[serde(default)]
    pub model_minimum_cli: BTreeMap<String, String>,
    #[serde(default)]
    pub direct_sqlite: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BreakingRange {
    pub range: String,
    pub reasons: Vec<CompatibilityReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityReason {
    SqliteSchemaConversion,
    EventRouteChanged,
    ServerPasswordRequired,
    VersionUnknown,
    VersionNotDeclared,
    InvalidManifest,
    UndeclaredCapability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityVerdict {
    Supported,
    Breaking,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatibilityEvaluation {
    pub verdict: CompatibilityVerdict,
    pub reasons: Vec<CompatibilityReason>,
}

impl CompatibilityEvaluation {
    pub fn allows_sqlite_read(&self) -> bool {
        self.verdict == CompatibilityVerdict::Supported
            || (self.verdict == CompatibilityVerdict::Unknown
                && self.reasons == [CompatibilityReason::VersionNotDeclared])
    }
}

impl Default for CompatibilityEvaluation {
    fn default() -> Self {
        unknown(CompatibilityReason::VersionUnknown)
    }
}

impl CompatibilityManifest {
    pub fn validate(&self) -> Result<(), String> {
        for range in self
            .supported
            .iter()
            .chain(self.breaking.iter().map(|r| &r.range))
        {
            VersionReq::parse(range)
                .map_err(|error| format!("invalid compatibility range {range}: {error}"))?;
        }
        for breaking in &self.breaking {
            if breaking.reasons.is_empty() {
                return Err("breaking compatibility range requires a reason".into());
            }
        }
        for version in self.model_minimum_cli.values() {
            Version::parse(version)
                .map_err(|error| format!("invalid model CLI minimum: {error}"))?;
        }
        Ok(())
    }

    pub fn evaluate(&self, detected: Option<&str>) -> CompatibilityEvaluation {
        if self.validate().is_err() {
            return unknown(CompatibilityReason::InvalidManifest);
        }
        let Some(version) = detected.and_then(|raw| Version::parse(raw).ok()) else {
            return unknown(CompatibilityReason::VersionUnknown);
        };
        let release = Version::new(version.major, version.minor, version.patch);
        let reasons: Vec<_> = self
            .breaking
            .iter()
            .filter(|rule| {
                VersionReq::parse(&rule.range).is_ok_and(|range| range.matches(&release))
            })
            .flat_map(|rule| rule.reasons.iter().copied())
            .fold(Vec::new(), |mut reasons, reason| {
                if !reasons.contains(&reason) {
                    reasons.push(reason);
                }
                reasons
            });
        if !reasons.is_empty() {
            return CompatibilityEvaluation {
                verdict: CompatibilityVerdict::Breaking,
                reasons,
            };
        }
        if self
            .supported
            .iter()
            .any(|rule| VersionReq::parse(rule).is_ok_and(|range| range.matches(&version)))
        {
            CompatibilityEvaluation {
                verdict: CompatibilityVerdict::Supported,
                reasons: Vec::new(),
            }
        } else {
            unknown(CompatibilityReason::VersionNotDeclared)
        }
    }
}

pub fn evaluate(
    manifest: Option<&CompatibilityManifest>,
    detected: Option<&str>,
) -> CompatibilityEvaluation {
    manifest.map_or_else(
        || unknown(CompatibilityReason::UndeclaredCapability),
        |m| m.evaluate(detected),
    )
}

fn unknown(reason: CompatibilityReason) -> CompatibilityEvaluation {
    CompatibilityEvaluation {
        verdict: CompatibilityVerdict::Unknown,
        reasons: vec![reason],
    }
}

#[cfg(test)]
mod tests;
