use super::*;
type Checkpoint = (Vec<u8>, Option<Vec<u8>>);
pub(super) fn checkpoint(
    tx: &mut Transaction<'_>,
    stamp: &str,
    key: &str,
) -> Result<Option<Checkpoint>, String> {
    Ok(tx
        .query_opt(
            "SELECT text_hash,vectors FROM mem_shadow WHERE target=$1 AND unit_key=$2",
            &[&stamp, &key],
        )
        .map_err(db_error)?
        .map(|r| (r.get(0), r.get(1))))
}
pub(super) fn vector_count(tx: &mut Transaction<'_>) -> Result<u64, String> {
    if vectors::stamp_in(tx)?.is_none() {
        return Ok(0);
    }
    super::count(tx, "mem_vectors")
}
impl PostgresMemStore {
    pub(super) fn publish_shadow_in(
        &self,
        target: &ActiveEmbedder,
        active: &ActiveEmbedder,
        stamp: &str,
        claim: &dyn Fn() -> bool,
    ) -> Result<bool, String> {
        let identity = target.identity();
        if stamp != serde_json::to_string(&identity).map_err(|e| e.to_string())? {
            return Err("shadow target stamp mismatch".into());
        }
        let width = identity
            .dimension
            .checked_mul(4)
            .filter(|w| *w > 0)
            .ok_or("invalid shadow vector dimension")?;
        let _gate = self.gate.lock();
        let published = self.transaction_locked(true, |tx| -> Result<bool, String> {
        let mut ready = Vec::new();
        for unit in targets::units(tx)? {
            match checkpoint(tx, stamp, &unit.key())? {
                Some((hash, bytes)) if hash == unit.hash() => ready.push((unit, bytes)),
                _ => return Ok(false),
            }
        }
        if !ready.is_empty() && ready.iter().all(|(_, v)| v.is_none()) {
            return Err("the target embedder produced no vectors".into());
        }
        for (unit, bytes) in &ready {
            if let Some(bytes) = bytes {
                if Some(bytes.len()) != unit.texts.len().checked_mul(width) {
                    return Err("shadow vector count or dimension mismatch".into());
                }
                vectors::decode(bytes)?;
            }
        }
        tx.batch_execute("DROP TABLE IF EXISTS mem_vectors; DELETE FROM mem_embedder; DELETE FROM mem_vector_pending; DELETE FROM blueprint_vectors").map_err(db_error)?;
        vectors::initialize_in(tx, &identity)?;
        for (unit, bytes) in ready {
            let Some(bytes) = bytes else {
                if !unit.blueprint {
                    super::mark(tx, &unit.id, "re-embed failed for this item")?;
                }
                continue;
            };
            for ((slot, ordinal, text), bytes) in unit.texts.iter().zip(bytes.chunks_exact(width)) {
                if unit.blueprint {
                    tx.execute("INSERT INTO blueprint_vectors(item_id,slot,ordinal,text,embedding,provider,model,dimension) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)", &[&unit.id,&slot,&(*ordinal as i64),&text,&bytes,&identity.provider,&identity.model,&(identity.dimension as i64)]).map_err(db_error)?;
                } else {
                    let vector = pgvector::Vector::from(vectors::decode(bytes)?);
                    tx.execute(
                        "INSERT INTO mem_vectors(mem_id,embedding) VALUES ($1,$2)",
                        &[
                            &codeterm_mem::embedder::index::vector_id(&unit.id, *ordinal),
                            &vector,
                        ],
                    )
                    .map_err(db_error)?;
                }
            }
        }
        if !claim() {
            return Err("re-embed cancelled before publication".into());
        }
        tx.execute("DELETE FROM mem_shadow", &[])
            .map_err(db_error)?;
        Ok(true)
        })?;
        if published {
            active.replace_silent(target.provider());
        }
        Ok(published)
    }
}
