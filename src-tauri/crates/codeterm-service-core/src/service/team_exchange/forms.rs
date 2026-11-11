//! Projects a checked package onto the form an export asks for. The `.team` and folder
//! forms carry the package as it is; the one-file form inlines every text reference and
//! refuses binary content with the form that can carry it.

use std::collections::BTreeMap;
use std::path::Path;

use codeterm_team_package::{
    frontmatter_name, InlineSkill, Package, PackageAutomation, PackageFormat, PackageSkill,
    SkillFile,
};

/// The bytes or folder one export writes.
pub enum Written {
    Bytes(Vec<u8>),
    Folder(Package),
}

fn within(path: &str, dir: &str) -> bool {
    path.strip_prefix(dir)
        .is_some_and(|rest| rest.starts_with('/'))
}

fn needs_archive(path: &str, why: &str) -> String {
    format!(
        "`{path}` {why}; the one-file form carries text only. Export with `--format team` or `--format folder`."
    )
}

fn text(path: &str, bytes: Vec<u8>) -> Result<String, String> {
    String::from_utf8(bytes).map_err(|_| needs_archive(path, "is binary"))
}

/// A top-level `key:` value from a small YAML or frontmatter block.
fn field(text: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    text.lines()
        .find_map(|line| line.strip_prefix(prefix.as_str()))
        .map(|value| value.trim().trim_matches(['"', '\'']).to_string())
        .filter(|value| !value.is_empty())
}

fn frontmatter(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("---") else {
        return "";
    };
    rest.split("\n---").next().unwrap_or_default()
}

/// The one-file form: every reference inline, no package entries.
pub fn single_file(package: &Package) -> Result<Package, String> {
    let mut package = package.clone();
    let mut entries = std::mem::take(&mut package.entries);
    if let Some(icon) = package.document.icon.clone() {
        if let Some(bytes) = entries.remove(&icon) {
            if icon.to_ascii_lowercase().ends_with(".png") {
                return Err(needs_archive(&icon, "is a PNG icon"));
            }
            let content = text(&icon, bytes)?;
            match package
                .document
                .files
                .iter_mut()
                .find(|file| file.path == icon)
            {
                Some(file) => file.content = Some(content),
                None => package
                    .document
                    .files
                    .push(codeterm_team_package::PackageFile {
                        path: icon,
                        description: None,
                        content: Some(content),
                    }),
            }
        }
    }
    for file in package.document.files.iter_mut() {
        if file.content.is_none() {
            let bytes = entries.remove(&file.path).unwrap_or_default();
            file.content = Some(text(&file.path, bytes)?);
        }
    }
    let mut instructions: BTreeMap<String, String> = BTreeMap::new();
    let names: Vec<(String, String)> = package
        .document
        .roles()
        .into_iter()
        .filter_map(|(_, name, role)| {
            role.instructions_file
                .clone()
                .map(|path| (name.to_string(), path))
        })
        .collect();
    for (_, path) in &names {
        if !instructions.contains_key(path) {
            let bytes = entries.remove(path).unwrap_or_default();
            instructions.insert(path.clone(), text(path, bytes)?);
        }
    }
    for (name, path) in names {
        let content = instructions.get(&path).cloned().unwrap_or_default();
        let role = match codeterm_team_package::ManagerSlot::parse(&name) {
            Some(slot) => match slot {
                codeterm_team_package::ManagerSlot::Orchestrator => {
                    Some(&mut package.document.managers.orchestrator)
                }
                codeterm_team_package::ManagerSlot::Planner => {
                    package.document.managers.planner.as_mut()
                }
                codeterm_team_package::ManagerSlot::Cartographer => {
                    package.document.managers.cartographer.as_mut()
                }
                codeterm_team_package::ManagerSlot::Curator => {
                    package.document.managers.curator.as_mut()
                }
            },
            None => package.document.workers.get_mut(&name),
        };
        if let Some(role) = role {
            role.instructions = Some(content);
            role.instructions_file = None;
        }
    }
    for skill in package.document.skills.iter_mut() {
        let PackageSkill::Path { path } = skill else {
            continue;
        };
        let dir = path.clone();
        let manifest = format!("{dir}/SKILL.md");
        let content = text(&manifest, entries.remove(&manifest).unwrap_or_default())?;
        let name = frontmatter_name(&content)
            .ok_or_else(|| format!("`{manifest}` needs a frontmatter `name`"))?;
        let description = field(frontmatter(&content), "description").unwrap_or_default();
        let inside: Vec<String> = entries
            .keys()
            .filter(|entry| within(entry, &dir))
            .cloned()
            .collect();
        let mut files = Vec::new();
        for entry in inside {
            let bytes = entries.remove(&entry).unwrap_or_default();
            let relative = entry[dir.len() + 1..].to_string();
            files.push(SkillFile {
                content: text(&entry, bytes)?,
                path: relative,
            });
        }
        *skill = PackageSkill::Inline(InlineSkill {
            name,
            description,
            content,
            files,
        });
    }
    for automation in package.document.automation.iter_mut() {
        let PackageAutomation::Path { path, description } = automation else {
            continue;
        };
        let (path, description) = (path.clone(), description.clone());
        let content = text(&path, entries.remove(path.as_str()).unwrap_or_default())?;
        let name = field(&content, "name").unwrap_or_else(|| {
            Path::new(path.as_str())
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone())
        });
        *automation = PackageAutomation::Inline {
            name,
            description,
            content,
        };
    }
    if let Some((path, _)) = entries.into_iter().next() {
        return Err(needs_archive(&path, "has no inline home"));
    }
    Ok(package)
}

/// The exact bytes or folder entries of `package` in `format`.
pub fn render(package: &Package, format: PackageFormat) -> Result<(Package, Written), String> {
    match format {
        PackageFormat::Yaml => {
            let inline = single_file(package)?;
            let bytes = inline.to_yaml().into_bytes();
            Ok((inline, Written::Bytes(bytes)))
        }
        PackageFormat::Team => {
            let bytes = package.to_archive()?;
            Ok((package.clone(), Written::Bytes(bytes)))
        }
        PackageFormat::Folder => Ok((package.clone(), Written::Folder(package.clone()))),
    }
}

/// The file name an export takes when the caller names no destination.
pub fn default_name(package: &Package, format: PackageFormat) -> String {
    let base: String = package
        .document
        .name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '-'
            }
        })
        .collect();
    let base = format!("{base}-{}", package.document.version);
    match format {
        PackageFormat::Yaml => format!("{base}.team.yaml"),
        PackageFormat::Team => format!("{base}.team"),
        PackageFormat::Folder => base,
    }
}

/// Writes a rendered package where the caller chose; a folder target must not exist or must
/// be empty.
pub fn write(target: &Path, written: &Written) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    match written {
        Written::Bytes(bytes) => codeterm_util::atomic_write::write_atomic(target, bytes)
            .map_err(|error| error.to_string()),
        Written::Folder(package) => {
            if target.exists()
                && std::fs::read_dir(target)
                    .map_err(|error| error.to_string())?
                    .next()
                    .is_some()
            {
                return Err(format!(
                    "`{}` already exists and is not empty; choose a new folder",
                    target.display()
                ));
            }
            package
                .write_folder(target)
                .map_err(|error| error.to_string())
        }
    }
}
