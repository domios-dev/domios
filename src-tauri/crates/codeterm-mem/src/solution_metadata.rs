use crate::{MemManager, NewMemItem};
use codeterm_blueprint_types::blueprint_memory::{CuratorEvidence, Entry, Evidence};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CuratorMetadata {
    pub confidence: Confidence,
    pub observed_at: i64,
    pub rounds: BTreeMap<String, u64>,
    pub episodes: BTreeSet<String>,
    pub grants: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlueprintMetadata {
    pub intents: Vec<String>,
    pub successes: u64,
    pub failures: u64,
    pub uses: u64,
    pub last_used: i64,
    pub explicitly_kept: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutcomeCounts {
    pub successes: u64,
    pub failures: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SolutionMetadata {
    pub content_fingerprint: Option<String>,
    pub blueprint: Option<BlueprintMetadata>,
    pub curator: Option<CuratorMetadata>,
    pub evidence: BTreeMap<String, OutcomeCounts>,
}

impl SolutionMetadata {
    pub fn set_entry(&mut self, entry: &Entry) {
        self.blueprint = Some(BlueprintMetadata {
            intents: entry.intents.clone(),
            successes: entry.successes,
            failures: entry.failures,
            uses: entry.uses,
            last_used: entry.last_used,
            explicitly_kept: entry.kept,
        });
    }

    pub fn prior(&self, origin: Option<&str>) -> OutcomeCounts {
        origin
            .and_then(|origin| self.evidence.get(origin))
            .cloned()
            .unwrap_or_default()
    }

    pub fn observe(&mut self, evidence: &Evidence, now: i64) {
        if let Some(origin) = &evidence.origin {
            let counts = self.evidence.entry(origin.clone()).or_default();
            counts.successes = counts.successes.max(evidence.successes);
            counts.failures = counts.failures.max(evidence.failures);
        }
        if let Some(proof) = &evidence.curator {
            self.observe_curator(proof, now);
        }
    }

    pub fn observe_curator(&mut self, proof: &CuratorEvidence, now: i64) {
        let curator = self.curator.get_or_insert_with(|| CuratorMetadata {
            confidence: Confidence::Low,
            observed_at: now,
            rounds: BTreeMap::new(),
            episodes: BTreeSet::new(),
            grants: BTreeSet::new(),
        });
        if curator
            .rounds
            .get(&proof.plan)
            .is_some_and(|round| *round < proof.round)
        {
            curator.confidence = match curator.confidence {
                Confidence::Low => Confidence::Medium,
                Confidence::Medium | Confidence::High => Confidence::High,
            };
        }
        curator.rounds.insert(proof.plan.clone(), proof.round);
        curator.observed_at = now;
        curator.episodes.extend(proof.episodes.iter().cloned());
        if proof.granted {
            curator
                .grants
                .insert(format!("{}:{}", proof.plan, proof.round));
        }
    }

    pub fn merge(&mut self, other: Self) {
        for (origin, counts) in other.evidence {
            let own = self.evidence.entry(origin).or_default();
            own.successes = own.successes.max(counts.successes);
            own.failures = own.failures.max(counts.failures);
        }
        if let Some(other) = other.curator {
            match &mut self.curator {
                None => self.curator = Some(other),
                Some(own) => {
                    own.confidence = match (own.confidence, other.confidence) {
                        (Confidence::High, _) | (_, Confidence::High) => Confidence::High,
                        (Confidence::Medium, _) | (_, Confidence::Medium) => Confidence::Medium,
                        _ => Confidence::Low,
                    };
                    own.observed_at = own.observed_at.max(other.observed_at);
                    for (plan, round) in other.rounds {
                        let own_round = own.rounds.entry(plan).or_default();
                        *own_round = (*own_round).max(round);
                    }
                    own.episodes.extend(other.episodes);
                    own.grants.extend(other.grants);
                }
            }
        }
    }
}

impl MemManager {
    pub fn solution_metadata(&self, id: &str) -> Result<Option<SolutionMetadata>, String> {
        self.store.solution_metadata(id)
    }

    pub fn curator_skill_covers(&self, id: &str, body: &str) -> Result<bool, String> {
        let record = self.solution_metadata(id)?;
        Ok(record
            .and_then(|record| record.content_fingerprint)
            .is_some_and(|fingerprint| {
                fingerprint
                    == codeterm_blueprint_types::blueprint_memory::curator::fingerprint(body)
            }))
    }

    pub fn save_curator_skill(
        &self,
        item: NewMemItem,
        improve: Option<&str>,
        proof: CuratorEvidence,
        now: i64,
    ) -> Result<String, String> {
        self.guard_commit()?;
        let ctx = Self::write_context();
        let author = ctx.default_system_author("curator");
        let id =
            self.store
                .curator_skill_save(&ctx.with_author(author), item, improve, proof, now)?;
        self.refresh_embedding(&id);
        Ok(id)
    }
}
#[cfg(test)]
mod tests;
