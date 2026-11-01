use codeterm_mem::backend::{ExportPage, MemStore, StoreRow};
use serde_json::{json, Value};
use std::collections::BTreeSet;

fn row_key(row: Option<&StoreRow>, index: usize) -> Value {
    let Some(row) = row else {
        return json!({"missing_row_at": index});
    };
    let keys: Vec<_> = [
        "id",
        "mem_id",
        "item_id",
        "singleton",
        "alias_id",
        "source_key",
        "lesson_uid",
        "slot",
        "ordinal",
        "hash",
        "path",
        "area",
        "team",
    ]
    .into_iter()
    .filter_map(|key| row.values.get(key).map(|value| (key, value)))
    .collect();
    json!({"index": index, "keys": keys})
}

fn row_difference(expected: Option<&StoreRow>, actual: Option<&StoreRow>, index: usize) -> Value {
    let columns: BTreeSet<_> = expected
        .into_iter()
        .chain(actual)
        .flat_map(|row| row.values.keys())
        .collect();
    for column in columns {
        let before = expected.and_then(|row| row.values.get(column));
        let after = actual.and_then(|row| row.values.get(column));
        if before != after {
            return json!({
                "row_key": {"expected": row_key(expected, index), "actual": row_key(actual, index)},
                "column": column, "expected": before, "actual": after,
            });
        }
    }
    json!({
        "row_key": {"expected": row_key(expected, index), "actual": row_key(actual, index)},
        "column": "<blobs>",
        "expected": expected.map(|row| &row.blobs),
        "actual": actual.map(|row| &row.blobs),
    })
}

fn first_difference(expected: &[ExportPage], actual: &[ExportPage]) -> Value {
    for index in 0..expected.len().max(actual.len()) {
        let before = expected.get(index);
        let after = actual.get(index);
        if before == after {
            continue;
        }
        let (Some(before), Some(after)) = (before, after) else {
            return json!({
                "table": before.map(|page| page.table).or_else(|| after.map(|page| page.table)),
                "row_key": {"page_index": index}, "column": "<page>",
                "expected": before, "actual": after,
            });
        };
        if before.table != after.table {
            return json!({
                "table": before.table, "row_key": {"page_index": index},
                "column": "<table>", "expected": before.table, "actual": after.table,
            });
        }
        for index in 0..before.rows.len().max(after.rows.len()) {
            let expected_row = before.rows.get(index);
            let actual_row = after.rows.get(index);
            if expected_row != actual_row {
                let mut difference = row_difference(expected_row, actual_row, index);
                difference["table"] = json!(before.table);
                return difference;
            }
        }
        return json!({
            "table": before.table, "row_key": {"page_index": index},
            "column": "<next_cursor>", "expected": before.next, "actual": after.next,
        });
    }
    json!({"column": "<none>"})
}

pub fn verify(
    store: &dyn MemStore,
    expected: &[ExportPage],
    name: &str,
    phase: &str,
) -> Result<(), String> {
    let actual = super::fixture::pages(store)?;
    if actual != expected {
        let mut difference = first_difference(expected, &actual);
        difference["store"] = json!(name);
        difference["phase"] = json!(phase);
        eprintln!("FIXTURE_MISMATCH {difference}");
        return Err(format!(
            "{name} {phase}: loaded store differs from the frozen corpus/metadata/vector rows"
        ));
    }
    println!(
        "FIXTURE_MATCH store={name} phase={phase} pages={}",
        actual.len()
    );
    Ok(())
}
