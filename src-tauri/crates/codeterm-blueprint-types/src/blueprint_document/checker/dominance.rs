use super::{engine::Database, Diagnostic, Finding, Proof, ProofAtom};
use crate::blueprint_document::Parsed;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub(super) fn check(parsed: &Parsed, db: &Database) -> Vec<Diagnostic> {
    let Some(entry) = db.rows("entry").next().and_then(|r| r.first()) else {
        return Vec::new();
    };
    let mut edges = BTreeMap::<&str, Vec<(&str, &str)>>::new();
    for edge in db.rows("edge") {
        edges
            .entry(&edge[0])
            .or_default()
            .push((&edge[1], &edge[2]));
    }
    let mut diagnostics = Vec::new();
    for bind in db.rows("bind") {
        let (consumer, slot, producer) = (&bind[0], &bind[1], &bind[2]);
        if !parsed.document.nodes().any(|n| n.id == *producer) || producer == entry {
            continue;
        }
        let mut paths = BTreeMap::new();
        let mut visited = BTreeSet::from([entry.as_str()]);
        let mut pending = VecDeque::from([entry.as_str()]);
        while let Some(node) = pending.pop_front() {
            for &(outcome, next) in edges.get(node).into_iter().flatten() {
                if next != producer && visited.insert(next) {
                    paths.insert(next, (node, outcome));
                    pending.push_back(next);
                }
            }
        }
        if !visited.contains(consumer.as_str()) {
            continue;
        }
        let Some(index) = parsed.document.nodes().position(|n| n.id == *consumer) else {
            continue;
        };
        let prefix = super::slots::node_path(&parsed.document, index);
        let field = if slot.starts_with("call.") || slot.starts_with("with.") {
            format!("{prefix}.{slot}")
        } else {
            format!("{prefix}.bind.{slot}")
        };
        let mut cursor = consumer.as_str();
        let mut path = Vec::new();
        while let Some(&(from, outcome)) = paths.get(cursor) {
            path.push(format!("{from}.{outcome} -> {cursor}"));
            cursor = from;
        }
        path.reverse();
        let message = format!(
            "Producer {producer} does not dominate {consumer}; branch path {} can skip {producer}",
            path.join(", ")
        );
        let atom = ProofAtom::new(
            "bind_not_dominated",
            vec![
                consumer.clone(),
                slot.clone(),
                producer.clone(),
                path.join(", "),
            ],
            true,
            &field,
            parsed.at(&field),
        );
        diagnostics.push(Diagnostic::from_proof(
            Finding {
                rule: "D1",
                name: "bind_not_dominated",
                severity: "error",
                message: &message,
                hint: "Move the producer before the branch, or consume it only on paths that execute it",
            },
            Proof { atom: atom.clone(), rule: "data_dominance".into(), children: Vec::new() },
            vec![atom],
            Vec::new(),
        ));
    }
    diagnostics
}
