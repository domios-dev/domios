use crate::backend::{ExportPage, ExportSnapshot, MemStore, StoreTable};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub type TableDigests = BTreeMap<StoreTable, String>;

pub(super) struct Contents(BTreeMap<StoreTable, Vec<[u8; 32]>>);

impl Contents {
    pub(super) fn new() -> Self {
        Self(
            StoreTable::ALL
                .into_iter()
                .map(|table| (table, Vec::new()))
                .collect(),
        )
    }

    pub(super) fn add(&mut self, page: &ExportPage, replace_singleton: bool) -> Result<(), String> {
        let hashes = self
            .0
            .get_mut(&page.table)
            .ok_or("memory_migration_table_inventory")?;
        if replace_singleton
            && !page.rows.is_empty()
            && matches!(
                page.table,
                StoreTable::Embedder | StoreTable::BlueprintCleanupReceipt
            )
        {
            hashes.clear();
        }
        for row in &page.rows {
            for (name, bytes) in &row.blobs {
                if *name != format!("sha256_{:x}.txt", Sha256::digest(bytes)) {
                    return Err("memory_migration_blob_digest_mismatch".into());
                }
            }
            let bytes = serde_json::to_vec(row).map_err(|error| error.to_string())?;
            hashes.push(Sha256::digest(bytes).into());
        }
        Ok(())
    }

    pub(super) fn finish(mut self) -> TableDigests {
        self.0
            .iter_mut()
            .map(|(table, rows)| {
                rows.sort_unstable();
                let mut digest = Sha256::new();
                for row in rows {
                    digest.update(row);
                }
                (*table, format!("{:x}", digest.finalize()))
            })
            .collect()
    }

    pub(super) fn read(snapshot: &dyn ExportSnapshot) -> Result<Self, String> {
        let mut contents = Self::new();
        let mut cursor = None;
        loop {
            let page = snapshot.export_page(cursor.as_ref(), 256)?;
            contents.add(&page, false)?;
            cursor = page.next;
            if cursor.is_none() {
                return Ok(contents);
            }
        }
    }
}

pub fn content_digests(store: &dyn MemStore) -> Result<TableDigests, String> {
    let mut result = None;
    store.with_export_snapshot(&mut |snapshot| {
        result = Some(Contents::read(snapshot)?.finish());
        Ok(())
    })?;
    result.ok_or("memory_migration_snapshot_missing".into())
}

pub fn verify_digests(expected: &TableDigests, actual: &TableDigests) -> Result<(), String> {
    if expected.len() != StoreTable::ALL.len() || actual.len() != StoreTable::ALL.len() {
        return Err("memory_migration_table_inventory: expected all 21 digests".into());
    }
    for table in StoreTable::ALL {
        if expected.get(&table).is_none() || expected.get(&table) != actual.get(&table) {
            return Err(format!("memory_migration_digest_mismatch: {table:?}"));
        }
    }
    Ok(())
}
