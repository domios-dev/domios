use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq)]
struct DocSection {
    doc_name: &'static str,
    section_title: String,
    anchor: String,
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
    "CONFIG",
    "NOTIFICATIONS",
    "INTEGRATIONS",
    "RUNTIME-CONFIG",
    "API",
    "API-TABS",
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
    "API-TEAMS",
    "API-TRANSPORT",
    "API-CLI",
    "TAB-LIFECYCLE",
    "MESH",
    "MESH-SERVICES",
    "MEMORY",
    "MEMORY-SERVICES",
    "MEMORY-USAGE",
    "HTML",
    "PROMPTS",
    "PROMPT-TIERS",
    "SKILLS",
    "AGENT-CONTROL",
    "PROMPT-DETECTION",
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

static INDEX: OnceLock<Index> = OnceLock::new();

pub fn search(query: &str, limit: usize) -> Vec<DocCandidate> {
    if limit == 0 {
        return Vec::new();
    }
    INDEX
        .get_or_init(|| {
            Index::from_sections(
                DOCS.iter()
                    .flat_map(|doc| chunks_for_document(doc.name, doc.body))
                    .chain(chunks_for_cli_reference(CLI_REFERENCE))
                    .collect(),
            )
        })
        .search(query, limit)
}

fn chunks_for_document(doc_name: &'static str, body: &str) -> Vec<DocSection> {
    chunks_for_headings(doc_name, body, 2, 3, true, false)
}

fn chunks_for_cli_reference(body: &str) -> Vec<DocSection> {
    let mut sections = chunks_for_headings("CLI", body, 4, 4, false, true);
    let mut parent_titles = BTreeSet::new();
    for section in &sections {
        let words: Vec<_> = section.section_title.split_whitespace().collect();
        for count in 2..words.len() {
            parent_titles.insert(words[..count].join(" "));
        }
    }
    sections.retain(|section| !parent_titles.contains(&section.section_title));
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
        if let Some(title) = section_heading(line, min_level, max_level) {
            let title = if trim_code_ticks {
                title.trim_start_matches('`').trim_end_matches('`')
            } else {
                title
            };
            if let Some(previous) = heading.replace(title.to_string()) {
                push_section(doc_name, previous, &mut lines, &mut anchors, &mut sections);
            } else if !keep_preamble {
                lines.clear();
            }
            continue;
        }
        if keep_preamble || heading.is_some() {
            lines.push(line);
        }
    }
    if let Some(title) = heading {
        push_section(doc_name, title, &mut lines, &mut anchors, &mut sections);
    } else if !lines.iter().all(|line| line.trim().is_empty()) {
        let text = lines.join("\n").trim().to_string();
        sections.push(DocSection {
            doc_name,
            section_title: "Overview".into(),
            anchor: slug(doc_name),
            text,
        });
    }
    sections
}

fn push_section(
    doc_name: &'static str,
    title: String,
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
        text,
    });
}

fn section_heading(line: &str, min_level: usize, max_level: usize) -> Option<&str> {
    let trimmed = line.trim_start();
    let level = trimmed.chars().take_while(|ch| *ch == '#').count();
    if !(min_level..=max_level).contains(&level) || trimmed.as_bytes().get(level) != Some(&b' ') {
        return None;
    }
    Some(trimmed[level..].trim().trim_end_matches('#').trim())
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
mod tests {
    use std::collections::BTreeSet;

    use super::{
        chunks_for_cli_reference, chunks_for_document, terms, DocSection, Index, CLI_REFERENCE,
        DOCS,
    };

    #[test]
    fn headings_split_sections_deterministically_and_ignore_fences() {
        let text = "# Guide\nOverview text.\n## First section\nFirst body.\n```md\n## Not a section\n```\n### Nested section\nNested body.\n## First section\nLast body.";
        let first = chunks_for_document("GUIDE", text);
        let second = chunks_for_document("GUIDE", text);
        assert_eq!(first, second);
        assert_eq!(first.len(), 3);
        assert_eq!(first[0].section_title, "First section");
        assert_eq!(first[0].anchor, "first-section");
        assert!(first[0].text.contains("Overview text."));
        assert!(first[0].text.contains("## Not a section"));
        assert_eq!(first[1].section_title, "Nested section");
        assert_eq!(first[2].anchor, "first-section-1");
    }

    #[test]
    fn bm25_returns_the_matching_test_section_first() {
        let index = Index::from_sections(vec![
            DocSection {
                doc_name: "CONFIG",
                section_title: "Secrets and unlock".into(),
                anchor: "secrets-and-unlock".into(),
                text: "The vault secret is unlocked with the stored key.".into(),
            },
            DocSection {
                doc_name: "MEMORY",
                section_title: "Search memory".into(),
                anchor: "search-memory".into(),
                text: "Memory candidates are recalled from the database.".into(),
            },
        ]);

        let result = index.search("vault unlock secret", 1);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].doc_name, "CONFIG");
        assert_eq!(result[0].anchor, "secrets-and-unlock");
    }

    #[test]
    fn embedded_doc_names_match_the_cli_registry() {
        const CLI_DOCS: &str = include_str!("../../../../cli/src/cmd/docs.rs");
        let registry = CLI_DOCS
            .split_once("pub(crate) const DOCS: &[EmbeddedDoc] = &[")
            .and_then(|(_, tail)| tail.split_once("\n];"))
            .map(|(registry, _)| registry)
            .expect("CLI docs registry");
        let cli_names = registry
            .split("doc!(")
            .skip(1)
            .map(|entry| {
                entry
                    .trim_start()
                    .strip_prefix('"')
                    .and_then(|name| name.split_once('"'))
                    .map(|(name, _)| name)
                    .expect("registered doc name")
            })
            .collect::<Vec<_>>();
        let daemon_names = DOCS.iter().map(|doc| doc.name).collect::<Vec<_>>();
        assert_eq!(daemon_names, cli_names);
    }

    #[test]
    fn generated_cli_reference_is_an_extra_command_chunk_corpus() {
        assert!(DOCS.iter().all(|doc| doc.name != "CLI"));
        let sections = chunks_for_cli_reference(CLI_REFERENCE);
        assert!(!sections.is_empty());
        assert!(sections.iter().all(|section| section.doc_name == "CLI"));
        assert!(sections
            .iter()
            .all(|section| section.section_title.starts_with("codeterm")));
        let titles: BTreeSet<_> = sections
            .iter()
            .map(|section| section.section_title.clone())
            .collect();
        assert!(sections.iter().all(|section| {
            let prefix = format!("{} ", section.section_title);
            !titles.iter().any(|title| title.starts_with(&prefix))
        }));

        let result = Index::from_sections(sections).search("codeterm sessions search", 1);

        assert_eq!(result[0].doc_name, "CLI");
        assert_eq!(result[0].section_title, "codeterm sessions search");
    }

    #[test]
    fn query_tokenization_is_case_insensitive() {
        assert_eq!(terms("Unlock Secret"), terms("unlock secret"));
    }
}
