use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

mod vectors;
#[cfg(all(test, feature = "embeddings"))]
pub(crate) use vectors::embed_section;
pub(crate) mod work;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VectorReadiness {
    Deferred,
    Starting,
    Ready,
    Failed,
}

fn vector_readiness() -> &'static tokio::sync::watch::Sender<VectorReadiness> {
    static STATE: OnceLock<tokio::sync::watch::Sender<VectorReadiness>> = OnceLock::new();
    STATE.get_or_init(|| tokio::sync::watch::channel(VectorReadiness::Deferred).0)
}

pub fn subscribe_vector_readiness() -> tokio::sync::watch::Receiver<VectorReadiness> {
    vector_readiness().subscribe()
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DocSection {
    doc_name: &'static str,
    section_title: String,
    anchor: String,
    heading_path: Vec<String>,
    text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DocCandidate {
    pub doc_name: &'static str,
    pub section_title: String,
    pub anchor: String,
    pub text: String,
    pub relevance: f64,
}

struct EmbeddedDoc {
    name: &'static str,
    body: &'static str,
}

const CLI_REFERENCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../generated/cli/cli.md"
));

macro_rules! embedded_docs {
    ($($name:literal),+ $(,)?) => {
        &[$(EmbeddedDoc {
            name: $name,
            body: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../prod-docs/", $name, ".md")),
        }),+]
    };
}

const DOCS: &[EmbeddedDoc] = embedded_docs![
    "ACP",
    "API-LIFECYCLE",
    "SPINAL-CORD",
    "BLUEPRINTS",
    "BLUEPRINT-AUTHORING",
    "BLUEPRINT-RECIPES",
    "CONFIG",
    "ACCOUNT",
    "NEIGHBORS",
    "NOTIFICATIONS",
    "INTEGRATIONS",
    "RUNTIME-CONFIG",
    "API",
    "API-TABS",
    "API-CHAT",
    "API-REPAIR",
    "API-ADOPT",
    "API-EVENTS",
    "API-PLAN",
    "MESH-BRIDGE",
    "API-INTEGRATIONS",
    "API-MEMORY",
    "API-SERVICES",
    "API-STATE",
    "API-AGENTS",
    "API-SPAWN",
    "PROVIDER-SWITCH",
    "API-TEAMS",
    "API-TRANSPORT",
    "API-CLI",
    "TAB-LIFECYCLE",
    "SURFACES",
    "MESH",
    "MESH-SERVICES",
    "MEMORY",
    "MEMORY-SERVICES",
    "MEMORY-USAGE",
    "HTML",
    "CHAT-BLOCKS",
    "SKILLS",
    "AGENT-CONTROL",
    "TELEGRAM-SETUP",
    "SECRETS",
    "ROUTING",
    "TEXT-HOOKS",
    "TRANSCRIBE",
    "ORCHESTRATION",
    "ORCHESTRATION-TASKS",
    "ORCHESTRATION-OPERATIONS",
    "PLAN-EDITING",
    "LANES",
    "PLANNER",
    "BOUNCE",
    "TEST-AUTHORING",
    "ESSENCE-MAP",
];

struct IndexedSection {
    section: DocSection,
    terms: HashMap<String, usize>,
    length: usize,
}

struct Index {
    sections: Vec<IndexedSection>,
    frequencies: HashMap<String, usize>,
    average_length: f64,
}

impl Index {
    fn from_sections(sections: Vec<DocSection>) -> Self {
        let indexed: Vec<IndexedSection> = sections
            .into_iter()
            .map(|section| {
                let terms = terms(&format!(
                    "{} {} {}",
                    section.doc_name, section.section_title, section.text
                ));
                let frequencies = term_frequencies(&terms);
                IndexedSection {
                    section,
                    terms: frequencies,
                    length: terms.len(),
                }
            })
            .collect();
        let mut frequencies = HashMap::new();
        for section in &indexed {
            for term in section.terms.keys() {
                *frequencies.entry(term.clone()).or_insert(0) += 1;
            }
        }
        let average_length = indexed.iter().map(|section| section.length).sum::<usize>() as f64
            / indexed.len().max(1) as f64;
        Self {
            sections: indexed,
            frequencies,
            average_length,
        }
    }

    fn search(&self, query: &str, limit: usize) -> Vec<DocCandidate> {
        if limit == 0 {
            return Vec::new();
        }
        let query_terms: BTreeSet<String> = terms(query).into_iter().collect();
        if query_terms.is_empty() {
            return Vec::new();
        }
        let total = self.sections.len() as f64;
        let mut ranked = self
            .sections
            .iter()
            .filter_map(|section| {
                let score = query_terms
                    .iter()
                    .filter_map(|term| {
                        let count = *section.terms.get(term)? as f64;
                        let document_frequency = *self.frequencies.get(term)? as f64;
                        let idf = (1.0
                            + (total - document_frequency + 0.5) / (document_frequency + 0.5))
                            .ln();
                        let norm = count
                            + 1.2
                                * (0.25
                                    + 0.75 * section.length as f64 / self.average_length.max(1.0));
                        Some(idf * count * 2.2 / norm)
                    })
                    .sum::<f64>();
                (score > 0.0).then_some((score, section))
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|(score_a, section_a), (score_b, section_b)| {
            score_b
                .total_cmp(score_a)
                .then_with(|| section_a.section.doc_name.cmp(section_b.section.doc_name))
                .then_with(|| section_a.section.anchor.cmp(&section_b.section.anchor))
        });
        ranked
            .into_iter()
            .take(limit)
            .map(|(relevance, indexed)| DocCandidate {
                doc_name: indexed.section.doc_name,
                section_title: indexed.section.section_title.clone(),
                anchor: indexed.section.anchor.clone(),
                text: indexed.section.text.clone(),
                relevance,
            })
            .collect()
    }
}

pub(crate) fn corpus_sections() -> &'static [DocSection] {
    static SECTIONS: OnceLock<Vec<DocSection>> = OnceLock::new();
    SECTIONS.get_or_init(|| {
        DOCS.iter()
            .flat_map(|doc| chunks_for_document(doc.name, doc.body))
            .chain(chunks_for_cli_reference(CLI_REFERENCE))
            .collect()
    })
}

fn index() -> &'static Index {
    static INDEX: OnceLock<Index> = OnceLock::new();
    INDEX.get_or_init(|| Index::from_sections(corpus_sections().to_vec()))
}

pub fn search(query: &str, limit: usize) -> Vec<DocCandidate> {
    if limit == 0 {
        return Vec::new();
    }
    index().search(query, limit)
}

pub fn search_with_hybrid(query: &str, limit: usize, enabled: bool) -> Vec<DocCandidate> {
    search_with_hybrid_start(query, limit, enabled, || {
        vectors::start_background_build(&codeterm_paths::data_dir());
    })
}

fn search_with_hybrid_start(
    query: &str,
    limit: usize,
    enabled: bool,
    start: impl FnOnce(),
) -> Vec<DocCandidate> {
    if !enabled || limit == 0 {
        return search(query, limit);
    }
    start();
    let Some((embedder, vectors)) = vectors::ready_vectors() else {
        return search(query, limit);
    };
    vectors::hybrid_search(index(), query, limit, &vectors, &|text| {
        embedder
            .query(text)
            .map(|vector| vector.vector)
            .ok_or_else(|| "docs query unavailable".into())
    })
}

impl crate::MemManager {
    pub(crate) fn docs_sections(&self) -> &[DocSection] {
        #[cfg(test)]
        if let Some(sections) = self.docs_sections.get() {
            return sections;
        }
        corpus_sections()
    }
}

pub(crate) fn reembed_vectors(
    data_dir: &std::path::Path,
    embedder: &std::sync::Arc<crate::embedder::ActiveEmbedder>,
    sections: &[DocSection],
) -> Result<usize, String> {
    vectors::reembed(data_dir, embedder, sections)
}

pub(crate) use vectors::PreparedDocs;

pub(crate) fn prepare_reembed(
    data_dir: &std::path::Path,
    embedder: &crate::embedder::ActiveEmbedder,
    sections: &[DocSection],
    progress: impl Fn(),
) -> Result<PreparedDocs, String> {
    vectors::prepare(data_dir, embedder, sections, progress)
}

#[cfg(test)]
pub(crate) fn fixture_sections() -> Vec<DocSection> {
    chunks_for_document(
        "FIXTURE",
        "# Guide\n## Search\nRecall stored notes.\n## Providers\nSwitch embedding models.\n## Persistence\nReopen the saved database.",
    )
}

pub(crate) fn publish_reembed(
    prepared: PreparedDocs,
    active: &std::sync::Arc<crate::embedder::ActiveEmbedder>,
) {
    vectors::publish(prepared, active);
}

pub fn prepare_hybrid_for_benchmark(data_dir: &std::path::Path) -> bool {
    vectors::prepare_for_benchmark(data_dir, corpus_sections())
}

fn chunks_for_document(doc_name: &'static str, body: &str) -> Vec<DocSection> {
    chunks_for_headings(doc_name, body, 2, 3, true, false)
}

fn chunks_for_cli_reference(body: &str) -> Vec<DocSection> {
    let mut sections = chunks_for_headings("CLI", body, 4, 4, false, true);
    let titles: Vec<String> = sections
        .iter()
        .map(|section| section.section_title.clone())
        .collect();
    sections.retain(|section| {
        !titles.iter().any(|title| {
            title
                .strip_prefix(&section.section_title)
                .is_some_and(|suffix| suffix.starts_with(' '))
        })
    });
    sections
}

fn chunks_for_headings(
    doc_name: &'static str,
    body: &str,
    min_level: usize,
    max_level: usize,
    keep_preamble: bool,
    trim_code_ticks: bool,
) -> Vec<DocSection> {
    let mut sections = Vec::new();
    let mut heading: Option<String> = None;
    let mut heading_path = Vec::<(usize, String)>::new();
    let mut section_path = Vec::<String>::new();
    let mut lines = Vec::new();
    let mut anchors = HashMap::<String, usize>::new();
    let mut fence: Option<(char, usize)> = None;
    for line in body.lines() {
        if let Some((marker, count)) = fence {
            lines.push(line);
            if closes_fence(line, marker, count) {
                fence = None;
            }
            continue;
        }
        if let Some((marker, count)) = opens_fence(line) {
            lines.push(line);
            fence = Some((marker, count));
            continue;
        }
        if let Some((level, raw_title)) = parsed_heading(line) {
            let selected = (min_level..=max_level).contains(&level);
            let title = raw_title;
            let title = if trim_code_ticks {
                title.trim_start_matches('`').trim_end_matches('`')
            } else {
                title
            };
            if selected {
                if let Some(previous) = heading.take() {
                    push_section(
                        doc_name,
                        previous,
                        &section_path,
                        &mut lines,
                        &mut anchors,
                        &mut sections,
                    );
                } else if !keep_preamble {
                    lines.clear();
                }
                update_heading_path(&mut heading_path, level, title);
                section_path = heading_path
                    .iter()
                    .map(|(_, title)| title.clone())
                    .collect();
                heading = Some(title.to_string());
                continue;
            }
            update_heading_path(&mut heading_path, level, title);
        }
        if keep_preamble || heading.is_some() {
            lines.push(line);
        }
    }
    if let Some(title) = heading {
        push_section(
            doc_name,
            title,
            &section_path,
            &mut lines,
            &mut anchors,
            &mut sections,
        );
    } else if !lines.iter().all(|line| line.trim().is_empty()) {
        let text = lines.join("\n").trim().to_string();
        let heading_path = if heading_path.is_empty() {
            vec!["Overview".into()]
        } else {
            heading_path
                .iter()
                .map(|(_, title)| title.clone())
                .collect()
        };
        sections.push(DocSection {
            doc_name,
            section_title: "Overview".into(),
            anchor: slug(doc_name),
            heading_path,
            text,
        });
    }
    sections
}

fn push_section(
    doc_name: &'static str,
    title: String,
    heading_path: &[String],
    lines: &mut Vec<&str>,
    anchors: &mut HashMap<String, usize>,
    sections: &mut Vec<DocSection>,
) {
    let text = std::mem::take(lines).join("\n").trim().to_string();
    if text.is_empty() {
        return;
    }
    let base = slug(&title);
    let duplicate = anchors.entry(base.clone()).or_insert(0);
    let anchor = if *duplicate == 0 {
        base
    } else {
        format!("{base}-{}", *duplicate)
    };
    *duplicate += 1;
    sections.push(DocSection {
        doc_name,
        section_title: title,
        anchor,
        heading_path: heading_path.to_vec(),
        text,
    });
}

fn update_heading_path(path: &mut Vec<(usize, String)>, level: usize, title: &str) {
    path.retain(|(existing_level, _)| *existing_level < level);
    path.push((level, title.to_string()));
}

fn parsed_heading(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    let level = trimmed.chars().take_while(|ch| *ch == '#').count();
    if !(1..=6).contains(&level) || trimmed.as_bytes().get(level) != Some(&b' ') {
        return None;
    }
    Some((level, trimmed[level..].trim().trim_end_matches('#').trim()))
}

fn opens_fence(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start();
    let marker = trimmed.chars().next()?;
    if !matches!(marker, '`' | '~') {
        return None;
    }
    let count = trimmed.chars().take_while(|ch| *ch == marker).count();
    (count >= 3).then_some((marker, count))
}

fn closes_fence(line: &str, marker: char, count: usize) -> bool {
    let trimmed = line.trim_start();
    let marker_count = trimmed.chars().take_while(|ch| *ch == marker).count();
    marker_count >= count && trimmed[marker_count..].trim().is_empty()
}

fn slug(text: &str) -> String {
    let mut result = String::new();
    for ch in text.chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() || ch == '_' || ch == '-' {
            result.push(ch);
        } else if ch.is_whitespace() && !result.ends_with('-') {
            result.push('-');
        }
    }
    result.trim_matches('-').to_string()
}

fn terms(text: &str) -> Vec<String> {
    text.split(|ch: char| !ch.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn term_frequencies(terms: &[String]) -> HashMap<String, usize> {
    let mut frequencies = HashMap::new();
    for term in terms {
        *frequencies.entry(term.clone()).or_insert(0) += 1;
    }
    frequencies
}

#[cfg(test)]
mod tests;
