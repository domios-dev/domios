use super::{bounded_text, clean, Digest, Row, MAX_DIGEST_BYTES};
use std::collections::BTreeMap;

struct SectionState {
    name: String,
    total: usize,
    emitted: usize,
    start: usize,
}

pub struct DigestBuilder {
    body: String,
    sizes: BTreeMap<String, usize>,
    limit: usize,
    activity: bool,
    section: Option<SectionState>,
}

pub fn is_activity_section(section: &str) -> bool {
    !matches!(
        section,
        "actionable"
            | "owner questions"
            | "waits"
            | "review backlog"
            | "time in state"
            | "agent capacity"
            | "queues"
    )
}

pub fn row_line(row: &Row) -> String {
    format!(
        "{} | {} | {} | {}\n",
        clean(&row.id, 64),
        clean(&row.kind, 32),
        clean(&row.state, 32),
        clean(&row.summary, 120)
    )
}

impl DigestBuilder {
    pub fn new(header: &str, limit: usize, activity: bool) -> Self {
        let limit = limit.clamp(1, MAX_DIGEST_BYTES);
        Self {
            body: format!("{}\n", bounded_text(header, limit.saturating_sub(1))),
            sizes: BTreeMap::new(),
            limit,
            activity,
            section: None,
        }
    }
    pub fn start_section(&mut self, name: &str, total: usize) -> bool {
        self.finish_section();
        let title = format!("\n{name}:\n");
        if self.body.len() + title.len() + 16 > self.limit {
            return false;
        }
        let start = self.body.len();
        self.body.push_str(&title);
        self.section = Some(SectionState {
            name: name.into(),
            total,
            emitted: 0,
            start,
        });
        true
    }
    pub fn append(&mut self, row: &Row) -> bool {
        let line = row_line(row);
        if self.body.len() + line.len() + 24 > self.limit {
            return false;
        }
        self.body.push_str(&line);
        if let Some(section) = self.section.as_mut() {
            section.emitted += 1;
        }
        true
    }
    fn finish_section(&mut self) {
        if let Some(section) = self.section.take() {
            if section.total > section.emitted {
                self.body
                    .push_str(&format!("+{} more\n", section.total - section.emitted));
            }
            self.sizes
                .insert(section.name, self.body.len() - section.start);
        }
    }
    pub fn finish(mut self) -> Digest {
        self.finish_section();
        Digest {
            body: self.body,
            activity: self.activity,
            section_sizes: self.sizes,
        }
    }
}
