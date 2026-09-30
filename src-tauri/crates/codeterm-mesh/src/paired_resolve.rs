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

/// One candidate considered when a target matched more than one paired record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairedNameCandidate {
    pub instance_id: String,
    pub instance_name: String,
    pub online: bool,
    pub relay_protocol_version: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvePairedError {
    /// No paired record matches this id, short id or name at all.
    NotFound,
    /// The name matched one or more paired records, but none is online —
    /// refusing to guess. Carries every candidate considered so the caller
    /// can report both records, and the target name so it can build the
    /// id-routed form of the same command.
    NoOnlineCandidate {
        name: String,
        candidates: Vec<PairedNameCandidate>,
    },
    /// More than one online record answers to the target; the caller must
    /// pick one by id instead of the resolver guessing.
    Ambiguous {
        name: String,
        candidates: Vec<PairedNameCandidate>,
    },
}

/// The advertised name without its trailing ` [dev-suffix]`, i.e. the name the
/// owner configured.
fn configured_name(advertised: &str) -> &str {
    match advertised.rfind(" [") {
        Some(at) if advertised.ends_with(']') => &advertised[..at],
        _ => advertised,
    }
}

fn candidate_of(peer: &PairedPeer) -> PairedNameCandidate {
    PairedNameCandidate {
        instance_id: peer.instance_id.clone(),
        instance_name: peer.instance_name.clone(),
        online: peer.online,
        relay_protocol_version: peer.relay_protocol_version,
    }
}

/// Peers whose id starts with `target` (the short id `mesh status` prints).
/// Only a hex-looking token of 6+ characters counts, so a name is never read
/// as an id prefix.
fn id_prefix_matches<'a>(
    peers: &'a HashMap<String, PairedPeer>,
    target: &str,
) -> Vec<&'a PairedPeer> {
    let hex_like = target.len() >= 6 && target.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    if !hex_like {
        return Vec::new();
    }
    let needle = target.to_ascii_lowercase();
    peers
        .values()
        .filter(|p| p.instance_id.to_ascii_lowercase().starts_with(&needle))
        .collect()
}

/// Peers answering to `target` by name: the full advertised name first, then
/// the configured name (advertised name minus its dev suffix).
fn name_matches<'a>(peers: &'a HashMap<String, PairedPeer>, target: &str) -> Vec<&'a PairedPeer> {
    let exact: Vec<&PairedPeer> = peers
        .values()
        .filter(|p| p.instance_name.eq_ignore_ascii_case(target))
        .collect();
    if !exact.is_empty() {
        return exact;
    }
    peers
        .values()
        .filter(|p| configured_name(&p.instance_name).eq_ignore_ascii_case(target))
        .collect()
}

/// Resolve a paired peer by instance_id (exact key), then unique short-id
/// prefix, then case-insensitive advertised or configured `instance_name`.
/// Stale offline duplicates of a name never hide the live record
/// (codeterm#434), but two online records under one name are refused as
/// `Ambiguous` rather than silently picking one.
pub fn resolve_paired_by_name<'a>(
    peers: &'a HashMap<String, PairedPeer>,
    target: &str,
) -> Result<&'a PairedPeer, ResolvePairedError> {
    if let Some(p) = peers.get(target) {
        return Ok(p);
    }

    let mut matches = id_prefix_matches(peers, target);
    if matches.is_empty() {
        matches = name_matches(peers, target);
    }
    if matches.is_empty() {
        return Err(ResolvePairedError::NotFound);
    }
    matches.sort_by(|a, b| a.instance_id.cmp(&b.instance_id));

    let online: Vec<&PairedPeer> = matches.iter().copied().filter(|p| p.online).collect();
    let candidates = |peers: &[&PairedPeer]| peers.iter().map(|p| candidate_of(p)).collect();
    match online.as_slice() {
        [only] => Ok(*only),
        [] => Err(ResolvePairedError::NoOnlineCandidate {
            name: target.to_string(),
            candidates: candidates(&matches),
        }),
        _ => Err(ResolvePairedError::Ambiguous {
            name: target.to_string(),
            candidates: candidates(&online),
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
    fn two_online_records_under_one_name_are_refused_with_candidates() {
        let mut peers = HashMap::new();
        peers.insert("id-1".into(), peer("id-1", "ROG", true, 1));
        peers.insert("id-2".into(), peer("id-2", "ROG", true, 2));
        match resolve_paired_by_name(&peers, "ROG").expect_err("ambiguous") {
            ResolvePairedError::Ambiguous { candidates, .. } => {
                let ids: Vec<&str> = candidates.iter().map(|c| c.instance_id.as_str()).collect();
                assert_eq!(ids, vec!["id-1", "id-2"]);
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn configured_name_and_short_id_resolve_a_dev_suffixed_peer() {
        let mut peers = HashMap::new();
        let full = "dc737738-aaaa-bbbb-cccc-000000000001";
        peers.insert(
            full.into(),
            peer(full, "BLACKSTAR-peer [dev-meshpeer]", true, 1),
        );
        peers.insert("3b6e44a3-x".into(), peer("3b6e44a3-x", "OTHER", true, 1));
        for target in [
            "BLACKSTAR-peer",
            "blackstar-peer [dev-meshpeer]",
            "dc737738",
            "DC737738",
        ] {
            let resolved = resolve_paired_by_name(&peers, target).expect(target);
            assert_eq!(resolved.instance_id, full, "{target}");
        }
    }

    #[test]
    fn a_short_id_shared_by_two_peers_is_ambiguous_and_a_name_is_never_an_id_prefix() {
        let mut peers = HashMap::new();
        peers.insert("abcdef01-1".into(), peer("abcdef01-1", "A", true, 1));
        peers.insert("abcdef01-2".into(), peer("abcdef01-2", "B", true, 1));
        assert!(matches!(
            resolve_paired_by_name(&peers, "abcdef"),
            Err(ResolvePairedError::Ambiguous { .. })
        ));
        peers.insert("dead0000-1".into(), peer("dead0000-1", "cafe", true, 1));
        let by_name = resolve_paired_by_name(&peers, "cafe").expect("name resolves");
        assert_eq!(by_name.instance_id, "dead0000-1");
    }

    #[test]
    fn a_stale_offline_duplicate_does_not_make_a_live_name_ambiguous() {
        let peers = duplicate_store();
        let resolved = resolve_paired_by_name(&peers, "BLACKSTAR").expect("resolves");
        assert_eq!(resolved.instance_id, "2556b3f9-live");
    }
}
