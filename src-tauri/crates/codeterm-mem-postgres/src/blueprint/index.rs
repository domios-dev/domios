use crate::{db_error, vectors};
use codeterm_mem::{
    backend::WriteContext,
    blueprint_library::index::{valid_dimension, IndexRow, Slot, VectorKey, Vectors},
    embedder::{ActiveEmbedder, EmbedderIdentity},
};
use postgres::Transaction;

pub(super) fn write_row(tx: &mut Transaction<'_>, id: &str, row: &IndexRow) -> Result<(), String> {
    let signature = serde_json::to_string(&row.signature).map_err(|error| error.to_string())?;
    let enrichment = row
        .enrichment
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| error.to_string())?;
    tx.execute("INSERT INTO blueprint_index(item_id,document_hash,canon,gists,signature_json,enrichment_json) VALUES ($1,$2,$3,$4,$5,$6) ON CONFLICT(item_id) DO UPDATE SET document_hash=excluded.document_hash,canon=excluded.canon,gists=excluded.gists,signature_json=excluded.signature_json,enrichment_json=excluded.enrichment_json", &[&id,&row.document_hash,&row.canon,&row.gists,&signature,&enrichment]).map_err(db_error)?;
    Ok(())
}

pub(super) fn read_row(tx: &mut Transaction<'_>, id: &str) -> Result<Option<IndexRow>, String> {
    tx.query_opt("SELECT document_hash,canon,gists,signature_json,enrichment_json FROM blueprint_index WHERE item_id=$1", &[&id]).map_err(db_error)?
        .map(|row| Ok(IndexRow {
            document_hash: row.get(0), canon: row.get(1), gists: row.get(2),
            signature: serde_json::from_str(&row.get::<_,String>(3)).map_err(|error| format!("invalid Blueprint signature for {id}: {error}"))?,
            enrichment: row.get::<_,Option<String>>(4).map(|json| serde_json::from_str(&json)).transpose().map_err(|error| format!("invalid Blueprint enrichment for {id}: {error}"))?,
        })).transpose()
}

pub(super) fn read_vectors(
    tx: &mut Transaction<'_>,
    id: &str,
    identity: &EmbedderIdentity,
) -> Result<Vectors, String> {
    let rows = tx.query("SELECT slot,ordinal,text,embedding FROM blueprint_vectors WHERE item_id=$1 AND provider=$2 AND model=$3 AND dimension=$4", &[&id,&identity.provider,&identity.model,&(identity.dimension as i64)]).map_err(db_error)?;
    let mut result = Vectors::default();
    for row in rows {
        let (Some(slot), Ok(ordinal)) = (
            Slot::parse(&row.get::<_, String>(0)),
            usize::try_from(row.get::<_, i64>(1)),
        ) else {
            continue;
        };
        let bytes: Vec<u8> = row.get(3);
        if let Ok(vector) = vectors::decode(&bytes) {
            if valid_dimension(&vector, identity.dimension) {
                result.by_slot.insert((slot, ordinal), (row.get(2), vector));
            }
        }
    }
    Ok(result)
}

pub(super) fn replace_vectors(
    tx: &mut Transaction<'_>,
    _ctx: &WriteContext,
    id: &str,
    embedder: &ActiveEmbedder,
    identity: &EmbedderIdentity,
    keep: &[VectorKey],
    written: &[(VectorKey, String, Vec<f32>)],
) -> Result<(), String> {
    if embedder.identity() != *identity {
        return Err("mixed-model Blueprint vectors refused".into());
    }
    let live: bool = tx
        .query_one(
            "SELECT EXISTS(SELECT 1 FROM blueprint_index WHERE item_id=$1)",
            &[&id],
        )
        .map_err(db_error)?
        .get(0);
    if !live {
        return Ok(());
    }
    tx.execute("DELETE FROM blueprint_vectors WHERE item_id=$1 AND (provider!=$2 OR model!=$3 OR dimension!=$4)", &[&id,&identity.provider,&identity.model,&(identity.dimension as i64)]).map_err(db_error)?;
    let existing = read_vectors(tx, id, identity)?;
    for key in existing.by_slot.keys() {
        if !keep.contains(key) {
            tx.execute(
                "DELETE FROM blueprint_vectors WHERE item_id=$1 AND slot=$2 AND ordinal=$3",
                &[&id, &key.0.name(), &(key.1 as i64)],
            )
            .map_err(db_error)?;
        }
    }
    for ((slot, ordinal), text, vector) in written {
        codeterm_mem::embedder::provider::validate_vector(vector, identity.dimension)
            .map_err(|error| error.to_string())?;
        let bytes: Vec<u8> = vector
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        tx.execute("INSERT INTO blueprint_vectors(item_id,slot,ordinal,text,embedding,provider,model,dimension) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT(item_id,slot,ordinal) DO UPDATE SET text=excluded.text,embedding=excluded.embedding,provider=excluded.provider,model=excluded.model,dimension=excluded.dimension", &[&id,&slot.name(),&(*ordinal as i64),&text,&bytes,&identity.provider,&identity.model,&(identity.dimension as i64)]).map_err(db_error)?;
    }
    Ok(())
}
