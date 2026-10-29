use super::{MemStore, StoreTable, TableCounts, WriteContext};
use std::{collections::BTreeSet, sync::Arc};

pub fn verify_counts(expected: &TableCounts, actual: &TableCounts) -> Result<(), String> {
    if expected.len() != StoreTable::ALL.len() || actual.len() != StoreTable::ALL.len() {
        return Err("memory_migration_table_inventory: expected all 21 tables".into());
    }
    for table in StoreTable::ALL {
        match (expected.get(&table), actual.get(&table)) {
            (Some(expected), Some(actual)) if expected == actual => {}
            _ => {
                return Err(format!(
                    "memory_migration_count_mismatch: {table:?}: expected {:?}, actual {:?}",
                    expected.get(&table),
                    actual.get(&table)
                ))
            }
        }
    }
    Ok(())
}

pub fn has_data(counts: &TableCounts) -> bool {
    counts.iter().any(|(table, count)| {
        *count > 0
            && !matches!(
                table,
                StoreTable::Embedder | StoreTable::BlueprintCleanupReceipt
            )
    })
}

pub fn transfer(
    source: &Arc<dyn MemStore>,
    target: &Arc<dyn MemStore>,
    into_nonempty: bool,
) -> Result<TableCounts, String> {
    if Arc::ptr_eq(source, target) {
        return Err("memory_migration_same_store: source and target must differ".into());
    }
    let source_counts = source.table_counts()?;
    let target_counts = target.table_counts()?;
    verify_counts(&source_counts, &source_counts)?;
    verify_counts(&target_counts, &target_counts)?;
    let populated = has_data(&target_counts);
    if populated && !into_nonempty {
        return Err(
            "memory_migration_target_nonempty: use --into-nonempty to authorize import".into(),
        );
    }
    if populated && target.vector_stamp()? != source.vector_stamp()? {
        return Err("memory_migration_target_stamp_conflict: non-empty target must use the source embedder stamp before import".into());
    }
    let mut expected = target_counts;
    for table in StoreTable::ALL {
        expected.insert(
            table,
            if matches!(
                table,
                StoreTable::Embedder | StoreTable::BlueprintCleanupReceipt
            ) {
                expected[&table].max(source_counts[&table])
            } else {
                expected[&table]
                    .checked_add(source_counts[&table])
                    .ok_or("memory_migration_count_overflow")?
            },
        );
    }
    let mut cursor = None;
    let mut seen = BTreeSet::new();
    let mut copied: TableCounts = StoreTable::ALL
        .into_iter()
        .map(|table| (table, 0))
        .collect();
    let ctx = WriteContext::system("backend-migrate");
    loop {
        let page = source.export_page(cursor.as_ref(), 256)?;
        let expected_table = cursor
            .as_ref()
            .map_or(StoreTable::Items, |cursor: &super::ExportCursor| {
                cursor.table
            });
        if page.table != expected_table {
            return Err("memory_migration_cursor: export returned a different table".into());
        }
        seen.insert(page.table);
        copied.insert(page.table, copied[&page.table] + page.rows.len() as u64);
        if let Some(next) = &page.next {
            if next.table < page.table
                || (next.table == page.table
                    && next.offset <= cursor.as_ref().map_or(0, |cursor| cursor.offset))
            {
                return Err("memory_migration_cursor: export failed to advance".into());
            }
        }
        target.import_page(&ctx, &page)?;
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    if seen.len() != StoreTable::ALL.len() {
        return Err("memory_migration_table_inventory: export skipped a table".into());
    }
    verify_counts(&source_counts, &copied)?;
    verify_counts(&source_counts, &source.table_counts()?)?;
    let counts = target.table_counts()?;
    verify_counts(&expected, &counts)?;
    Ok(counts)
}
