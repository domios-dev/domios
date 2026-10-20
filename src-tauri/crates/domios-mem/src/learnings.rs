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
}
