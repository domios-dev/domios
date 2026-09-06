//! Deterministic name resolution over `paired_peers`.
//!
//! A peer that re-mints its `instance_id` (fresh identity after a reinstall,
//! a wiped `~/.codeterm/mesh/identity.json`, etc.) re-pairs under the SAME
//! `instance_name` while the OLD record — now permanently offline — can be
//! left behind under its old id. `MeshManager::complete_explicit_pairing`
//! supersedes that old record when it can (same name, same host), but a
//! duplicate that predates this fix, or one whose host changed too, can
//! still exist on disk. Name-based routing must give ONE deterministic
//! answer over such a store, never a `HashMap`-iteration-order coin flip
//! (codeterm#434).

use std::collections::HashMap;

use super::types::PairedPeer;

/// One candidate considered when a name matched more than one paired record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairedNameCandidate {
    pub instance_id: String,
    pub online: bool,
    pub relay_protocol_version: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvePairedError {
    /// No paired record matches this id or name at all.
    NotFound,
    /// The name matched one or more paired records, but none is online —
    /// refusing to guess. Carries every candidate considered so the caller
    /// can report both records, and the target name so it can build the
    /// id-routed form of the same command.
    NoOnlineCandidate {
        name: String,
        candidates: Vec<PairedNameCandidate>,
    },
}

/// Resolve a paired peer by instance_id first (exact key match), then by
/// case-insensitive `instance_name`. When several paired records share the
/// name, prefer the ONLINE record with the highest `relay_protocol_version`
/// — the same answer on every call, independent of hashmap iteration order.
/// Refuses only when no online candidate exists for the name at all.
pub fn resolve_paired_by_name<'a>(
    peers: &'a HashMap<String, PairedPeer>,
    target: &str,
) -> Result<&'a PairedPeer, ResolvePairedError> {
    if let Some(p) = peers.get(target) {
        return Ok(p);
    }

    let mut matches: Vec<&PairedPeer> = peers
        .values()
        .filter(|p| p.instance_name.eq_ignore_ascii_case(target))
        .collect();
    if matches.is_empty() {
        return Err(ResolvePairedError::NotFound);
    }
    // Deterministic tie-break before picking a winner, so two online peers
    // at the same protocol version don't depend on hashmap order either.
    matches.sort_by(|a, b| a.instance_id.cmp(&b.instance_id));

    let best = matches
        .iter()
        .filter(|p| p.online)
        .max_by_key(|p| p.relay_protocol_version);

    match best {
        Some(winner) => Ok(*winner),
        None => Err(ResolvePairedError::NoOnlineCandidate {
            name: target.to_string(),
            candidates: matches
                .iter()
                .map(|p| PairedNameCandidate {
                    instance_id: p.instance_id.clone(),
                    online: p.online,
                    relay_protocol_version: p.relay_protocol_version,
                })
                .collect(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AccessLevel, PeerAddress};

    fn peer(id: &str, name: &str, online: bool, protocol: u16) -> PairedPeer {
        PairedPeer {
            instance_id: id.into(),
            instance_name: name.into(),
            addresses: vec![PeerAddress::from_ip("100.64.1.9")],
            api_port: 7685,
            public_key: "pub".into(),
            paired_at: "now".into(),
            last_seen: None,
            online,
            is_primary: false,
            access_level: AccessLevel::Full,
            version: None,
            web_ui: None,
            relay_protocol_version: protocol,
            consecutive_misses: 0,
            last_active_ms: None,
        }
    }

    fn duplicate_store() -> HashMap<String, PairedPeer> {
        let mut peers = HashMap::new();
        peers.insert(
            "a4db6f3d-dead".into(),
            peer("a4db6f3d-dead", "BLACKSTAR", false, 0),
        );
        peers.insert(
            "2556b3f9-live".into(),
            peer("2556b3f9-live", "BLACKSTAR", true, 1),
        );
        peers
    }

    /// codeterm#434: repeated resolution over the SAME duplicate store must
    /// always return the online record — never flip based on iteration order.
    #[test]
    fn resolves_the_online_record_every_time_over_a_duplicate_store() {
        let peers = duplicate_store();
        for _ in 0..200 {
            let resolved = resolve_paired_by_name(&peers, "BLACKSTAR").expect("resolves");
            assert_eq!(resolved.instance_id, "2556b3f9-live");
            assert!(resolved.online);
        }
        // Case-insensitive, like the pre-existing id-based check.
        for _ in 0..50 {
            let resolved = resolve_paired_by_name(&peers, "blackstar").expect("resolves");
            assert_eq!(resolved.instance_id, "2556b3f9-live");
        }
    }

    #[test]
    fn id_lookup_wins_even_when_the_id_looks_like_another_peers_name() {
        let peers = duplicate_store();
        let resolved =
            resolve_paired_by_name(&peers, "a4db6f3d-dead").expect("exact id key resolves");
        assert_eq!(resolved.instance_id, "a4db6f3d-dead");
    }

    #[test]
    fn refuses_and_names_both_records_when_no_online_candidate_exists() {
        let mut peers = HashMap::new();
        peers.insert(
            "a4db6f3d-dead".into(),
            peer("a4db6f3d-dead", "BLACKSTAR", false, 0),
        );
        peers.insert(
            "c9112abc-alsodead".into(),
            peer("c9112abc-alsodead", "BLACKSTAR", false, 1),
        );

        let error =
            resolve_paired_by_name(&peers, "BLACKSTAR").expect_err("no online record exists");
        match error {
            ResolvePairedError::NoOnlineCandidate { name, candidates } => {
                assert_eq!(name, "BLACKSTAR");
                let ids: Vec<&str> = candidates.iter().map(|c| c.instance_id.as_str()).collect();
                assert!(ids.contains(&"a4db6f3d-dead"), "got: {ids:?}");
                assert!(ids.contains(&"c9112abc-alsodead"), "got: {ids:?}");
                assert_eq!(candidates.len(), 2);
                assert!(candidates.iter().all(|c| !c.online));
            }
            other => panic!("expected NoOnlineCandidate, got {other:?}"),
        }
    }

    #[test]
    fn unknown_name_is_not_found() {
        let peers = duplicate_store();
        assert!(matches!(
            resolve_paired_by_name(&peers, "NOPE"),
            Err(ResolvePairedError::NotFound)
        ));
    }

    #[test]
    fn highest_protocol_wins_among_multiple_online_candidates() {
        let mut peers = HashMap::new();
        peers.insert("id-1".into(), peer("id-1", "ROG", true, 1));
        peers.insert("id-2".into(), peer("id-2", "ROG", true, 2));
        let resolved = resolve_paired_by_name(&peers, "ROG").expect("resolves");
        assert_eq!(resolved.instance_id, "id-2");
    }
}
