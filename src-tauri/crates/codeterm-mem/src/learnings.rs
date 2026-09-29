//! Learnings capture pipeline. Quick `mem learn` writes a kind=learning
//! mem_item with low-friction defaults. `learn list` filters by kind=learning.
//! `learn promote <id>` rewrites the row's kind to `fact` (preserving body
//! and graph relations) so it gets included in dreaming consolidation.

use super::{store::MemKind, MemManager, MemScope, NewMemItem};

#[derive(Debug, Clone)]
pub enum LearnKind {
    Insight,
    Error,
    Feature,
}

impl LearnKind {
    fn tag(&self) -> &'static str {
        match self {
            Self::Insight => "insight",
            Self::Error => "error",
            Self::Feature => "feature",
        }
    }
}

pub fn learn(
    mem: &MemManager,
    text: &str,
    learn_kind: LearnKind,
    extra_tags: Vec<String>,
) -> Result<String, String> {
    let mut tags = vec!["learning".to_string(), learn_kind.tag().to_string()];
    tags.extend(extra_tags);
    let title = format!(
        "{} {}",
        learn_kind.tag(),
        text.lines()
            .next()
            .unwrap_or("untitled")
            .chars()
            .take(60)
            .collect::<String>()
    );
    mem.save(NewMemItem {
        kind: MemKind::Learning,
        scope: MemScope::Global,
        title,
        summary: None,
        area: None,
        tags,
        body: text.to_string(),
        source: Some("cli:learn".into()),
        importance: 3.5,
        source_path: None,
        pinned: false,
    })
}

pub fn promote(mem: &MemManager, id: &str) -> Result<bool, String> {
    mem.promote_learning(id).map(|_| true)
}

/// Largest body `mem save` accepts, in bytes.
pub const MAX_BODY_BYTES: usize = 1_048_576;

/// Tag set on a flush whose scrollback exceeded `MAX_BODY_BYTES`.
pub const TRUNCATED_TAG: &str = "truncated";

/// The last `max_bytes` of `body` at most, starting on a char boundary.
fn keep_tail(body: &str, max_bytes: usize) -> &str {
    if body.len() <= max_bytes {
        return body;
    }
    let mut start = body.len() - max_bytes;
    while !body.is_char_boundary(start) {
        start += 1;
    }
    &body[start..]
}

/// Grab a tab's PTY scrollback and store as kind=conversation mem_item.
/// Called from PreCompact hook or tab lifecycle exit. Scrollback over the save
/// cap keeps its most recent tail and is tagged `truncated`.
pub fn flush_tab(
    mem: &MemManager,
    tab_id: &str,
    title_hint: &str,
    body: String,
) -> Result<String, String> {
    if body.trim().is_empty() {
        return Err("empty body — nothing to flush".into());
    }
    let title = format!("flush:{} {}", tab_id, title_hint);
    let mut tags = vec![
        "flush".to_string(),
        "conversation".to_string(),
        format!("tab:{}", tab_id),
    ];
    let mut summary = format!("PTY scrollback flush from {}", tab_id);
    let kept = keep_tail(&body, MAX_BODY_BYTES);
    if kept.len() < body.len() {
        tags.push(TRUNCATED_TAG.to_string());
        summary.push_str(&format!(
            " (tail kept: {} of {} bytes)",
            kept.len(),
            body.len()
        ));
    }
    let body = if kept.len() < body.len() {
        kept.to_string()
    } else {
        body
    };
    mem.save(NewMemItem {
        kind: MemKind::Conversation,
        scope: MemScope::Global,
        title,
        summary: Some(summary),
        area: None,
        tags,
        body,
        source: Some(format!("tab:{}", tab_id)),
        importance: 2.5,
        source_path: None,
        pinned: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learn_creates_learning_item() {
        let mem = MemManager::open_in_memory().unwrap();
        let id = learn(&mem, "fixed bug X by doing Y", LearnKind::Insight, vec![]).unwrap();
        let item = mem.get(&id).unwrap().unwrap();
        assert_eq!(item.kind, "learning");
        assert!(item.tags.iter().any(|t| t == "insight"));
    }

    #[test]
    fn promote_changes_kind_to_fact() {
        let mem = MemManager::open_in_memory().unwrap();
        let id = learn(&mem, "API timeout fix", LearnKind::Insight, vec![]).unwrap();
        promote(&mem, &id).unwrap();
        let after = mem.get(&id).unwrap().unwrap();
        assert_eq!(after.kind, "fact");
        assert!(after.importance >= 4.0);
    }

    #[test]
    fn promote_rejects_non_learning() {
        let mem = MemManager::open_in_memory().unwrap();
        let id = mem
            .save(NewMemItem {
                kind: MemKind::Note,
                scope: MemScope::Global,
                title: "x".into(),
                summary: None,
                area: None,
                tags: vec![],
                body: "y".into(),
                source: None,
                importance: 3.0,
                source_path: None,
                pinned: false,
            })
            .unwrap();
        let r = promote(&mem, &id);
        assert!(r.is_err());
    }

    #[test]
    fn flush_tab_creates_conversation() {
        let mem = MemManager::open_in_memory().unwrap();
        let id = flush_tab(&mem, "p123", "deploy run", "lots of output here".into()).unwrap();
        let item = mem.get(&id).unwrap().unwrap();
        assert_eq!(item.kind, "conversation");
        assert!(item.source.unwrap().contains("p123"));
    }

    #[test]
    fn flush_respects_body_cap() {
        let mem = MemManager::open_in_memory().unwrap();
        let head = "HEAD-SENTINEL ";
        let tail = " TAIL-SENTINEL";
        let body = format!("{head}{}{tail}", "漢🦀 scrollback line\n".repeat(100_000));
        assert!(body.len() > 2 * MAX_BODY_BYTES);

        let id = flush_tab(&mem, "p9", "big", body.clone()).unwrap();
        let item = mem.get(&id).unwrap().unwrap();

        assert!(item.body.len() <= MAX_BODY_BYTES);
        assert!(body.ends_with(item.body.as_str()));
        assert!(item.body.ends_with(tail));
        assert!(!item.body.contains(head));
        assert!(item.tags.iter().any(|t| t == TRUNCATED_TAG));
    }

    #[test]
    fn flush_under_the_cap_is_stored_whole() {
        let mem = MemManager::open_in_memory().unwrap();
        let body = "short scrollback".to_string();
        let id = flush_tab(&mem, "p8", "small", body.clone()).unwrap();
        let item = mem.get(&id).unwrap().unwrap();
        assert_eq!(item.body, body);
        assert!(!item.tags.iter().any(|t| t == TRUNCATED_TAG));
    }

    #[test]
    fn keep_tail_lands_on_a_char_boundary() {
        let body = "🦀".repeat(10);
        for max in 0..=body.len() {
            let kept = keep_tail(&body, max);
            assert!(kept.len() <= max);
            assert!(body.ends_with(kept));
        }
    }

    #[test]
    fn flush_rejects_empty() {
        let mem = MemManager::open_in_memory().unwrap();
        assert!(flush_tab(&mem, "p", "", "".into()).is_err());
    }
}
