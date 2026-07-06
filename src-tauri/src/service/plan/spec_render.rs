use crate::plan::{Section, Spec};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SpecFileEntry {
    pub kind: &'static str,
    pub title: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SpecFileManifest {
    pub index_path: String,
    pub entries: Vec<SpecFileEntry>,
}

pub fn spec_body_dir(spec: &Spec) -> std::io::Result<PathBuf> {
    super::paths::specs_dir(canonical_project_path(spec)).map(|dir| dir.join(&spec.id))
}

pub fn section_path(spec: &Spec, section_id: &str) -> std::io::Result<PathBuf> {
    let handle = flatten_sections(&spec.sections)
        .into_iter()
        .find(|section| section.id == section_id)
        .map(super::tag_model::section_handle)
        .unwrap_or(section_id);
    Ok(spec_body_dir(spec)?
        .join("sections")
        .join(format!("{handle}.md")))
}

pub fn index_path(spec: &Spec) -> std::io::Result<PathBuf> {
    Ok(spec_body_dir(spec)?.join("INDEX.md"))
}

pub fn strip_bodies(spec: &Spec) -> Spec {
    let mut stripped = spec.clone();
    clear_section_bodies(&mut stripped.sections);
    stripped
}

pub fn persist_bodies(spec: &Spec) -> std::io::Result<Vec<PathBuf>> {
    let paths = body_paths(spec)?;
    for section in flatten_sections(&spec.sections) {
        let path = section_path(spec, &section.id)?;
        super::body_files::write_markdown_atomic(
            &path,
            &body_with_frontmatter(&section.tags, &section.body),
        )?;
        remove_stale_id_path(spec, &section.id, &path)?;
    }
    write_index(spec)?;
    Ok(paths)
}

pub fn body_paths(spec: &Spec) -> std::io::Result<Vec<PathBuf>> {
    let mut paths = vec![];
    collect_section_paths(spec, &spec.sections, &mut paths)?;
    paths.push(index_path(spec)?);
    Ok(paths)
}

pub fn file_manifest(spec: &Spec) -> std::io::Result<SpecFileManifest> {
    let project = canonical_project_path(spec);
    let entries = index_entries(spec)?
        .into_iter()
        .map(|entry| SpecFileEntry {
            kind: entry.kind,
            title: entry.title,
            path: project_relative(project, &entry.path),
        })
        .collect();
    Ok(SpecFileManifest {
        index_path: project_relative(project, &index_path(spec)?),
        entries,
    })
}

fn canonical_project_path(spec: &Spec) -> &Path {
    if spec.canonical_project_path.trim().is_empty() {
        Path::new(&spec.project_path)
    } else {
        Path::new(&spec.canonical_project_path)
    }
}

pub fn spec_markdown(spec: &Spec) -> String {
    let mut out = String::new();
    render_sections(&spec.sections, 1, &mut out);
    out
}

pub fn hydrate_and_migrate(spec: &mut Spec) -> std::io::Result<bool> {
    let mut migrated = false;
    super::tag_model::normalize_sections_for_spec(&mut spec.sections);
    let base_dir = spec_body_dir(spec)?;
    hydrate_sections(&base_dir, &mut spec.sections, &mut migrated)?;
    Ok(migrated)
}

pub fn remove_body_dir(spec: &Spec) -> std::io::Result<()> {
    let dir = spec_body_dir(spec)?;
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

fn collect_section_paths(
    spec: &Spec,
    sections: &[Section],
    paths: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    for section in sections {
        paths.push(section_path(spec, &section.id)?);
        collect_section_paths(spec, &section.subsections, paths)?;
    }
    Ok(())
}

fn flatten_sections(sections: &[Section]) -> Vec<&Section> {
    let mut out = vec![];
    for section in sections {
        out.push(section);
        out.extend(flatten_sections(&section.subsections));
    }
    out
}

struct IndexEntryRaw {
    kind: &'static str,
    title: String,
    path: PathBuf,
    index_link_path: String,
}

fn index_entries(spec: &Spec) -> std::io::Result<Vec<IndexEntryRaw>> {
    let mut entries = vec![];
    collect_section_index_entries(spec, &spec.sections, &mut entries)?;
    Ok(entries)
}

fn collect_section_index_entries(
    spec: &Spec,
    sections: &[Section],
    entries: &mut Vec<IndexEntryRaw>,
) -> std::io::Result<()> {
    for section in sections {
        entries.push(IndexEntryRaw {
            kind: "section",
            title: format!(
                "{} ({})",
                super::tag_model::section_handle(section),
                section.title
            ),
            path: section_path(spec, &section.id)?,
            index_link_path: format!("sections/{}.md", super::tag_model::section_handle(section)),
        });
        collect_section_index_entries(spec, &section.subsections, entries)?;
    }
    Ok(())
}

fn write_index(spec: &Spec) -> std::io::Result<()> {
    let entries = index_entries(spec)?;
    let mut out = format!("# {}\n\n", spec.title);
    out.push_str("## Sections\n\n");
    if entries.is_empty() {
        out.push_str("- (none)\n");
    } else {
        for entry in entries {
            out.push_str(&format!(
                "- [{}]({})\n",
                escape_markdown_link_text(&entry.title),
                entry.index_link_path
            ));
        }
    }
    super::body_files::write_markdown_atomic(&index_path(spec)?, &out)
}

fn render_sections(sections: &[Section], depth: usize, out: &mut String) {
    for section in sections {
        let level = "#".repeat(depth.min(6));
        out.push_str(&format!("{level} {}\n\n", section.title));
        if !section.body.is_empty() {
            out.push_str(&section.body);
            out.push_str("\n\n");
        }
        render_sections(&section.subsections, depth + 1, out);
    }
}

fn hydrate_sections(
    base_dir: &Path,
    sections: &mut [Section],
    migrated: &mut bool,
) -> std::io::Result<()> {
    for section in sections {
        let path = base_dir
            .join("sections")
            .join(format!("{}.md", super::tag_model::section_handle(section)));
        let fallback = base_dir.join("sections").join(format!("{}.md", section.id));
        *migrated |= hydrate_body(&path, Some(&fallback), &mut section.body, &mut section.tags)?;
        hydrate_sections(base_dir, &mut section.subsections, migrated)?;
    }
    Ok(())
}

fn hydrate_body(
    path: &Path,
    fallback: Option<&Path>,
    body: &mut String,
    tags: &mut Vec<String>,
) -> std::io::Result<bool> {
    let had_inline_body = !body.is_empty();
    let read_path = if path.exists() {
        Some(path)
    } else {
        fallback.filter(|fallback| fallback.exists())
    };
    if let Some(read_path) = read_path {
        let text = std::fs::read_to_string(read_path)?;
        let (frontmatter_tags, stripped) = parse_frontmatter_tags(&text);
        if !frontmatter_tags.is_empty() {
            *tags = frontmatter_tags;
        }
        *body = stripped;
        return Ok(had_inline_body);
    }
    if had_inline_body {
        super::body_files::write_markdown_atomic(path, &body_with_frontmatter(tags, body))?;
        return Ok(true);
    }
    Ok(false)
}

fn clear_section_bodies(sections: &mut [Section]) {
    for section in sections {
        section.body.clear();
        clear_section_bodies(&mut section.subsections);
    }
}

fn remove_stale_id_path(spec: &Spec, id: &str, current: &Path) -> std::io::Result<()> {
    let stale = spec_body_dir(spec)?
        .join("sections")
        .join(format!("{id}.md"));
    if stale != current && stale.exists() {
        std::fs::remove_file(stale)?;
    }
    Ok(())
}

fn body_with_frontmatter(tags: &[String], body: &str) -> String {
    if tags.is_empty() {
        return body.to_string();
    }
    let yaml = serde_yaml::to_string(&serde_json::json!({ "tags": tags })).unwrap_or_default();
    format!("---\n{}---\n\n{}", yaml, body.trim_start())
}

fn parse_frontmatter_tags(text: &str) -> (Vec<String>, String) {
    let Some(rest) = text.strip_prefix("---\n") else {
        return (vec![], text.to_string());
    };
    let Some(end) = rest.find("\n---") else {
        return (vec![], text.to_string());
    };
    let yaml = &rest[..end];
    let body = rest[end + "\n---".len()..].trim_start_matches('\n');
    let tags = serde_yaml::from_str::<serde_yaml::Value>(yaml)
        .ok()
        .and_then(|value| value.get("tags").cloned())
        .and_then(|value| serde_yaml::from_value::<Vec<String>>(value).ok())
        .unwrap_or_default();
    (tags, body.to_string())
}

fn escape_markdown_link_text(text: &str) -> String {
    text.replace('\\', "\\\\").replace(']', "\\]")
}

fn project_relative(project: &Path, path: &Path) -> String {
    path.strip_prefix(project)
        .unwrap_or(path)
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}
