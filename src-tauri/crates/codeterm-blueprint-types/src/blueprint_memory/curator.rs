use std::collections::BTreeSet;

pub struct Candidate {
    pub shape: String,
    pub episodes: BTreeSet<String>,
}

pub fn fingerprint(text: &str) -> String {
    let hash = text.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    format!("{hash:016x}")
}

#[derive(Debug, PartialEq, Eq)]
pub enum Relation {
    Unrelated,
    Covers(String),
    Conflict(String),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Save,
    Improve(String),
}

pub fn decide(
    episodes: &BTreeSet<String>,
    granted: bool,
    already_saved: bool,
    hits: &[Relation],
) -> Result<Decision, String> {
    if already_saved {
        return Err("one save or improve per Curator round".into());
    }
    if !granted && episodes.iter().filter(|id| !id.trim().is_empty()).count() < 2 {
        return Err("two distinct closed episodes or a save grant required".into());
    }
    if let Some(id) = hits.iter().take(3).find_map(|hit| match hit {
        Relation::Conflict(id) => Some(id),
        _ => None,
    }) {
        return Err(format!("conflict with {id}"));
    }
    Ok(hits
        .iter()
        .take(3)
        .find_map(|hit| match hit {
            Relation::Covers(id) => Some(Decision::Improve(id.clone())),
            _ => None,
        })
        .unwrap_or(Decision::Save))
}

pub fn confidence(previous_round: Option<u64>, round: u64, previous: &str) -> &'static str {
    match (previous_round, previous) {
        (Some(prior), "low") if prior < round => "medium",
        (Some(prior), "medium" | "high") if prior < round => "high",
        (_, "medium") => "medium",
        (_, "high") => "high",
        _ => "low",
    }
}

pub fn dream_candidate(curator: bool, confidence: &str, observed_at: i64, now: i64) -> bool {
    curator && confidence == "low" && now.saturating_sub(observed_at) >= 60 * 86400
}

#[cfg(test)]
mod tests;
