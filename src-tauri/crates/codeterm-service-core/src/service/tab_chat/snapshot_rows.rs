use crate::agent::chat_types::{ChatMsg, RichMessage};

pub const SNAPSHOT_ROW_LIMIT: usize = 150;

pub fn retained_group_start(counts: &[usize], limit: usize) -> usize {
    let mut start = counts.len();
    let mut total: usize = 0;
    for index in (0..counts.len()).rev() {
        if total > 0 && total.saturating_add(counts[index]) > limit {
            break;
        }
        total = total.saturating_add(counts[index]);
        start = index;
    }
    start
}

pub fn retain_tail(rows: &[RichMessage], read_start: u64) -> (u64, Vec<ChatMsg>) {
    let mut groups: Vec<(u64, Vec<ChatMsg>)> = Vec::new();
    for (ordinal, row) in rows.iter().enumerate() {
        let start = row.record_start.unwrap_or(read_start);
        if groups.last().is_none_or(|group| group.0 != start) {
            groups.push((start, vec![]));
        }
        groups
            .last_mut()
            .unwrap()
            .1
            .extend(super::rich_to_chat_msgs_at(
                row,
                super::RowOrigin::at(read_start, ordinal),
            ));
    }
    let counts: Vec<usize> = groups.iter().map(|group| group.1.len()).collect();
    let first = retained_group_start(&counts, SNAPSHOT_ROW_LIMIT);
    let start = groups.get(first).map_or(read_start, |group| group.0);
    (
        start,
        groups
            .into_iter()
            .skip(first)
            .flat_map(|group| group.1)
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_retention_keeps_source_records_whole_and_never_strands_rows() {
        assert_eq!(retained_group_start(&[80, 70, 40], 150), 1);
        assert_eq!(retained_group_start(&[80, 70], 150), 0);
        assert_eq!(retained_group_start(&[10, 151], 150), 1);
        assert_eq!(retained_group_start(&[151, 10], 150), 1);
        assert_eq!(retained_group_start(&[10, 151, 0], 150), 1);
        assert_eq!(retained_group_start(&[], 150), 0);
    }

    fn rows(start: u64, count: usize) -> Vec<RichMessage> {
        (0..count)
            .map(|index| RichMessage {
                role: "assistant".into(),
                record_start: Some(start),
                record_row: index as u32,
                blocks: vec![crate::agent::chat_types::MessageBlock {
                    kind: "text".into(),
                    data: serde_json::json!({"text":"supplied-content"}),
                }],
                ..Default::default()
            })
            .collect()
    }

    #[test]
    fn oversized_record_rows_are_whole_and_exact_limit_records_both_survive() {
        let rich: Vec<_> = rows(0, 10).into_iter().chain(rows(100, 151)).collect();
        let (start, retained) = retain_tail(&rich, 0);
        assert_eq!(start, 100);
        assert_eq!(retained.len(), 151);
        let rich: Vec<_> = rows(0, 50).into_iter().chain(rows(100, 100)).collect();
        let (start, retained) = retain_tail(&rich, 0);
        assert_eq!(start, 0);
        assert_eq!(retained.len(), 150);
    }
}
