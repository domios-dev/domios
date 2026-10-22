use super::*;

pub(super) struct UnitChanges {
    pub row_changes: Vec<Unit>,
    pub embedding_changes: Vec<Unit>,
    pub stale_vectors: HashSet<String>,
}

pub(super) fn unit_changes(existing: &HashMap<String, IndexedUnit>, units: &[Unit]) -> UnitChanges {
    let next_ids = units
        .iter()
        .map(|unit| unit.id.clone())
        .collect::<HashSet<_>>();
    let row_changes = units
        .iter()
        .filter(|unit| existing.get(&unit.id) != Some(&unit_signature(unit)))
        .cloned()
        .collect::<Vec<_>>();
    let embedding_changes = row_changes
        .iter()
        .filter(|unit| {
            existing
                .get(&unit.id)
                .map(|signature| signature.body != unit.body || signature.title != unit.title)
                .unwrap_or(true)
        })
        .cloned()
        .collect::<Vec<_>>();
    let stale_vectors = existing
        .keys()
        .filter(|id| !next_ids.contains(*id))
        .chain(embedding_changes.iter().map(|unit| &unit.id))
        .cloned()
        .collect::<HashSet<_>>();
    UnitChanges {
        row_changes,
        embedding_changes,
        stale_vectors,
    }
}
