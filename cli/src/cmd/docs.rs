//! Embedded CodeTerm docs. Bundled at compile time via include_str! so the
//! CLI ships with them — agents can `codeterm docs <name>` on demand without
//! polluting the system prompt.

use std::fmt::Write;

pub(crate) const DOCS: &[(&str, &str)] = &[
    ("CONFIG", include_str!("../../../docs/CONFIG.md")),
    ("API", include_str!("../../../docs/API.md")),
    ("MESH", include_str!("../../../docs/MESH.md")),
    ("MEMORY", include_str!("../../../docs/MEMORY.md")),
    ("BUILD", include_str!("../../../docs/BUILD.md")),
    ("HTML", include_str!("../../../docs/HTML.md")),
    ("PROMPTS", include_str!("../../../docs/PROMPTS.md")),
    ("SKILLS", include_str!("../../../docs/SKILLS.md")),
    ("TESTING", include_str!("../../../docs/TESTING.md")),
    (
        "TELEGRAM-SETUP",
        include_str!("../../../docs/TELEGRAM-SETUP.md"),
    ),
    ("SECRETS", include_str!("../../../docs/SECRETS.md")),
    ("ROUTING", include_str!("../../../docs/ROUTING.md")),
    ("TEXT-HOOKS", include_str!("../../../docs/TEXT-HOOKS.md")),
    ("TRANSCRIBE", include_str!("../../../docs/TRANSCRIBE.md")),
    (
        "ORCHESTRATION",
        include_str!("../../../docs/ORCHESTRATION.md"),
    ),
    (
        "PLAN-EDITING",
        include_str!("../../../docs/PLAN-EDITING.md"),
    ),
    ("LANES", include_str!("../../../docs/LANES.md")),
    ("PLANNER", include_str!("../../../docs/PLANNER.md")),
    (
        "BOUNCE",
        include_str!("../../../src-tauri/prompts/bounce_unwinding_contract.md"),
    ),
    (
        "TEST-AUTHORING",
        include_str!("../../../docs/contributing/test-authoring.md"),
    ),
];

const SECTION_INDEX_MIN_LINES: usize = 120;
const DESCRIPTION_MAX_CHARS: usize = 96;

pub fn handle(name: Option<String>, full: bool, head: Option<usize>, section: Option<String>) {
    match name {
        None => list_docs(),
        Some(name) => match render_doc(&name, full, head, section.as_deref()) {
            Ok(rendered) => print!("{}", rendered),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        },
    }
}

pub fn handle_doc(name: String) {
    let resolved = if name.eq_ignore_ascii_case("orchestration") {
        "ORCHESTRATION"
    } else {
        name.as_str()
    };
    match render_doc(resolved, true, None, None) {
        Ok(rendered) => print!("{}", rendered),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

fn list_docs() {
    println!("Available docs (use `codeterm docs <name>` to read):");
    for (name, body) in DOCS {
        let lines = body.lines().count();
        let title = document_title(body).unwrap_or("(no heading)");
        println!("  {:<16} {} lines  — {}", name, lines, title);
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DocError {
    UnknownDocument {
        requested: String,
    },
    ConflictingSelectors,
    UnknownSection {
        document: String,
        requested: String,
        available: Vec<String>,
    },
    AmbiguousSection {
        document: String,
        requested: String,
        available: Vec<String>,
    },
    InvalidHead {
        document: String,
        requested: usize,
        source_lines: usize,
    },
}

impl std::fmt::Display for DocError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownDocument { requested } => {
                writeln!(formatter, "No doc named '{requested}'. Available:")?;
                for (name, _) in DOCS {
                    writeln!(formatter, "  {name}")?;
                }
                Ok(())
            }
            Self::ConflictingSelectors => write!(
                formatter,
                "Choose exactly one docs selector: `--section <slug>`, `--full`, or `--head <lines>`."
            ),
            Self::UnknownSection {
                document,
                requested,
                available,
            } => write_section_error(
                formatter,
                "No section",
                requested,
                document,
                available,
            ),
            Self::AmbiguousSection {
                document,
                requested,
                available,
            } => {
                writeln!(
                    formatter,
                    "Ambiguous section '{requested}' in {document}: normalized slug '{}' matches multiple headings.",
                    normalize_slug(requested)
                )?;
                write_available_sections(formatter, document, available)
            }
            Self::InvalidHead {
                document,
                requested,
                source_lines,
            } => write!(
                formatter,
                "`--head {requested}` cannot produce an incomplete excerpt from {document} ({source_lines} source lines). Choose a value from 1 to {} or read `codeterm docs {document} --full`.",
                source_lines.saturating_sub(1)
            ),
        }
    }
}

fn write_section_error(
    formatter: &mut std::fmt::Formatter<'_>,
    kind: &str,
    requested: &str,
    document: &str,
    available: &[String],
) -> std::fmt::Result {
    writeln!(formatter, "{kind} '{requested}' in {document}.")?;
    write_available_sections(formatter, document, available)
}

fn write_available_sections(
    formatter: &mut std::fmt::Formatter<'_>,
    document: &str,
    available: &[String],
) -> std::fmt::Result {
    writeln!(formatter, "Available section slugs:")?;
    if available.is_empty() {
        writeln!(formatter, "  (none; this document has no level-2 sections)")?;
    } else {
        for slug in available {
            writeln!(
                formatter,
                "  {slug} — `codeterm docs {document} --section {slug}`"
            )?;
        }
    }
    write!(
        formatter,
        "Complete document: `codeterm docs {document} --full`."
    )
}

pub(crate) fn render_doc(
    name: &str,
    full: bool,
    head: Option<usize>,
    section: Option<&str>,
) -> Result<String, DocError> {
    if usize::from(full) + usize::from(head.is_some()) + usize::from(section.is_some()) > 1 {
        return Err(DocError::ConflictingSelectors);
    }

    let upper = name.to_ascii_uppercase();
    let Some((document, body)) = DOCS.iter().find(|(document, _)| *document == upper) else {
        return Err(DocError::UnknownDocument {
            requested: name.to_string(),
        });
    };

    if full {
        return Ok((*body).to_string());
    }
    if let Some(limit) = head {
        return render_head(document, body, limit);
    }

    let sections = document_sections(body);
    if let Some(requested) = section {
        return render_section(document, body, &sections, requested);
    }
    if body.lines().count() >= SECTION_INDEX_MIN_LINES && sections.len() >= 2 {
        return Ok(render_index(document, body, &sections));
    }
    Ok((*body).to_string())
}

fn render_head(name: &str, body: &str, limit: usize) -> Result<String, DocError> {
    let lines: Vec<&str> = body.lines().collect();
    if limit == 0 || limit >= lines.len() {
        return Err(DocError::InvalidHead {
            document: name.to_string(),
            requested: limit,
            source_lines: lines.len(),
        });
    }
    let mut output = lines[..limit].join("\n");
    output.push_str(&format!(
        "\n\n[INCOMPLETE diagnostic excerpt: first {limit}/{} source lines from {name}. Read one complete section with `codeterm docs {name} --section <slug>` or the byte-exact document with `codeterm docs {name} --full`.]\n",
        lines.len()
    ));
    Ok(output)
}

fn render_index(name: &str, body: &str, sections: &[DocSection]) -> String {
    let title = document_title(body).unwrap_or(name);
    let mut output = format!(
        "# {title}\n\nSection index for {name} ({} source lines). Each command returns one complete section:\n\n",
        body.lines().count()
    );
    for section in sections {
        let _ = writeln!(
            output,
            "- `{}` ({} lines) — {}\n  `codeterm docs {name} --section {}`",
            section.slug, section.line_count, section.description, section.slug
        );
    }
    let _ = write!(
        output,
        "\nComplete byte-exact document: `codeterm docs {name} --full`.\n"
    );
    output
}

fn render_section(
    document: &str,
    body: &str,
    sections: &[DocSection],
    requested: &str,
) -> Result<String, DocError> {
    let requested_slug = normalize_slug(requested);
    let matches: Vec<&DocSection> = sections
        .iter()
        .filter(|section| section.slug == requested_slug)
        .collect();
    let available = available_slugs(sections);
    match matches.as_slice() {
        [section] => Ok(body[section.start..section.end].to_string()),
        [] => Err(DocError::UnknownSection {
            document: document.to_string(),
            requested: requested.to_string(),
            available,
        }),
        _ => Err(DocError::AmbiguousSection {
            document: document.to_string(),
            requested: requested.to_string(),
            available,
        }),
    }
}

fn available_slugs(sections: &[DocSection]) -> Vec<String> {
    let mut counts = std::collections::BTreeMap::new();
    for section in sections {
        *counts.entry(section.slug.clone()).or_insert(0_usize) += 1;
    }
    counts
        .into_iter()
        .filter_map(|(slug, count)| (count == 1).then_some(slug))
        .collect()
}

#[derive(Debug)]
struct DocSection {
    slug: String,
    start: usize,
    end: usize,
    line_count: usize,
    description: String,
}

#[derive(Clone, Copy, Debug)]
struct Heading<'a> {
    level: usize,
    title: &'a str,
    start: usize,
}

fn document_sections(body: &str) -> Vec<DocSection> {
    let headings = markdown_headings(body);
    headings
        .iter()
        .enumerate()
        .filter(|(_, heading)| heading.level == 2)
        .map(|(index, heading)| {
            let end = headings[index + 1..]
                .iter()
                .find(|next| next.level <= heading.level)
                .map_or(body.len(), |next| next.start);
            let content = &body[heading.start..end];
            DocSection {
                slug: normalize_slug(heading.title),
                start: heading.start,
                end,
                line_count: content.lines().count(),
                description: section_description(content),
            }
        })
        .collect()
}

fn markdown_headings(body: &str) -> Vec<Heading<'_>> {
    let mut headings = Vec::new();
    let mut offset = 0;
    let mut fence: Option<(char, usize)> = None;
    for line_with_ending in body.split_inclusive('\n') {
        let line = line_with_ending.trim_end_matches(['\r', '\n']);
        if let Some(marker) = fence_marker(line) {
            match fence {
                Some(open) if marker.0 == open.0 && marker.1 >= open.1 => fence = None,
                None => fence = Some(marker),
                _ => {}
            }
        } else if fence.is_none() {
            if let Some((level, title)) = atx_heading(line) {
                headings.push(Heading {
                    level,
                    title,
                    start: offset,
                });
            }
        }
        offset += line_with_ending.len();
    }
    headings
}

fn fence_marker(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start();
    let marker = trimmed.chars().next()?;
    if !matches!(marker, '`' | '~') {
        return None;
    }
    let count = trimmed.chars().take_while(|character| *character == marker).count();
    (count >= 3).then_some((marker, count))
}

fn atx_heading(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    let level = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    if !(1..=6).contains(&level)
        || !matches!(trimmed.as_bytes().get(level), Some(b' ' | b'\t'))
    {
        return None;
    }
    let title = trimmed[level..].trim();
    let title = title.trim_end_matches('#').trim_end();
    (!title.is_empty()).then_some((level, title))
}

fn document_title(body: &str) -> Option<&str> {
    markdown_headings(body)
        .into_iter()
        .find(|heading| heading.level == 1)
        .map(|heading| heading.title)
}

fn normalize_slug(value: &str) -> String {
    let mut slug = String::new();
    let mut separator_pending = false;
    for character in value.trim().chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() {
            if separator_pending && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(character);
            separator_pending = false;
        } else {
            separator_pending = true;
        }
    }
    slug
}

fn section_description(content: &str) -> String {
    let description = content
        .lines()
        .skip(1)
        .map(str::trim)
        .find(|line| {
            !line.is_empty()
                && !line.starts_with("```")
                && !line.starts_with("~~~")
                && !line.starts_with("<!-- revision:")
        })
        .unwrap_or("No description.");
    truncate_chars(description, DESCRIPTION_MAX_CHARS)
}

fn truncate_chars(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_string();
    }
    let mut truncated: String = value.chars().take(limit.saturating_sub(1)).collect();
    truncated.push('…');
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORCHESTRATION: &str = include_str!("../../../docs/ORCHESTRATION.md");

    #[test]
    fn long_doc_default_is_compact_section_index_not_prefix_preview() {
        let rendered = render_doc("ORCHESTRATION", false, None, None).expect("doc exists");
        let sections = document_sections(ORCHESTRATION);

        for section in &sections {
            assert!(rendered.contains(&format!("`{}`", section.slug)));
            assert!(rendered.contains(&format!(
                "`codeterm docs ORCHESTRATION --section {}`",
                section.slug
            )));
        }
        assert!(rendered.starts_with("# Orchestration control loop\n"));
        assert!(!rendered.contains("codeterm orchestrator status --plan"));
        assert!(!rendered.contains("output truncated"));
        assert!(!rendered.contains("showing 120/"));
        assert!(rendered.lines().count() <= sections.len() * 2 + 7);
        assert!(rendered.len() < 8_000);
    }

    #[test]
    fn docs_section_reads_complete_named_section_in_one_call() {
        let start = ORCHESTRATION.find("## Dispatch contract").unwrap();
        let end = ORCHESTRATION.find("## Review capacity").unwrap();
        let expected = &ORCHESTRATION[start..end];
        let rendered = render_doc(
            "ORCHESTRATION",
            false,
            None,
            Some("dispatch-contract"),
        )
        .expect("section exists");

        assert_eq!(rendered, expected);
        assert!(rendered.starts_with("## Dispatch contract"));
        assert!(rendered.trim_end().ends_with("complete lane lifecycle."));
        assert!(!rendered.contains("## Review capacity"));
        assert!(!rendered.contains("INCOMPLETE"));
    }

    #[test]
    fn docs_full_remains_byte_exact_complete_document() {
        let rendered = render_doc("ORCHESTRATION", true, None, None).expect("doc exists");

        assert_eq!(rendered.as_bytes(), ORCHESTRATION.as_bytes());
    }

    #[test]
    fn explicit_head_is_labelled_incomplete() {
        let rendered = render_doc("ORCHESTRATION", false, Some(12), None).expect("doc exists");
        let marker = "\n\n[INCOMPLETE diagnostic excerpt:";
        let (excerpt, label) = rendered.split_once(marker).expect("incomplete label");
        let expected = ORCHESTRATION.lines().take(12).collect::<Vec<_>>().join("\n");

        assert_eq!(excerpt, expected);
        assert!(label.contains("first 12/"));
        assert!(label.contains("--section <slug>"));
        assert!(label.contains("--full"));
    }

    #[test]
    fn docs_section_parser_accepts_case_insensitive_slug() {
        let rendered = render_doc(
            "orchestration",
            false,
            None,
            Some("DiSpAtCh CoNtRaCt"),
        )
        .expect("normalized heading matches");

        assert!(rendered.starts_with("## Dispatch contract"));
    }

    #[test]
    fn docs_section_parser_rejects_unknown_ambiguous_and_conflicting_selectors() {
        let unknown = render_doc("ORCHESTRATION", false, None, Some("does-not-exist"))
            .expect_err("unknown section fails");
        let unknown_message = unknown.to_string();
        assert!(unknown_message.contains("dispatch-contract"));
        assert!(unknown_message.contains(
            "`codeterm docs ORCHESTRATION --section dispatch-contract`"
        ));
        assert!(unknown_message.contains("`codeterm docs ORCHESTRATION --full`"));

        let duplicate = "# Fixture\n\n## Same heading\nfirst\n\n## Same-heading\nsecond\n";
        let sections = document_sections(duplicate);
        let ambiguous = render_section("FIXTURE", duplicate, &sections, "SAME HEADING")
            .expect_err("duplicate normalized headings fail");
        assert!(matches!(ambiguous, DocError::AmbiguousSection { .. }));
        assert!(ambiguous.to_string().contains("same-heading"));

        let no_sections = render_doc("LANES", false, None, Some("missing"))
            .expect_err("non-sectioned short doc fails");
        assert!(no_sections.to_string().contains("(none;"));
        assert!(no_sections.to_string().contains("codeterm docs LANES --full"));

        for error in [
            render_doc("ORCHESTRATION", true, None, Some("dispatch-contract")),
            render_doc("ORCHESTRATION", false, Some(5), Some("dispatch-contract")),
        ] {
            assert_eq!(error, Err(DocError::ConflictingSelectors));
        }
    }

    #[test]
    fn structural_parser_ignores_fenced_headings_and_uses_same_or_higher_boundary() {
        let body = "# Fixture\n\n## One\nlead\n```bash\n# not a title\n## not a section\n```\n### Child\nchild\n## Two\nend\n";
        let sections = document_sections(body);

        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].slug, "one");
        assert_eq!(&body[sections[0].start..sections[0].end], "## One\nlead\n```bash\n# not a title\n## not a section\n```\n### Child\nchild\n");
        assert_eq!(sections[1].slug, "two");
    }

    #[test]
    fn short_non_sectioned_docs_remain_complete_by_default() {
        let full_body = DOCS.iter().find(|(name, _)| *name == "LANES").unwrap().1;
        let rendered = render_doc("LANES", false, None, None).expect("doc exists");

        assert_eq!(rendered, full_body);
    }

    #[test]
    fn bounce_doc_preserves_registered_source() {
        let source = include_str!("../../../src-tauri/prompts/bounce_unwinding_contract.md");
        let rendered = render_doc("bounce", true, None, None).expect("doc exists");

        assert_eq!(rendered, source);
    }
}
