//! Projects a checked package onto the form an export asks for. The `.team` and folder
//! forms carry the package as it is; the one-file form inlines every text reference and
//! refuses binary content with the form that can carry it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use codeterm_team_package::{
    frontmatter_name, InlineSkill, ManagerSlot, Package, PackageAutomation, PackageFormat,
    PackageSkill, SkillFile, TeamDocument, TeamRole,
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

fn role_mut<'a>(document: &'a mut TeamDocument, name: &str) -> Option<&'a mut TeamRole> {
    match ManagerSlot::parse(name) {
        Some(ManagerSlot::Orchestrator) => Some(&mut document.managers.orchestrator),
        Some(ManagerSlot::Planner) => document.managers.planner.as_mut(),
        Some(ManagerSlot::Cartographer) => document.managers.cartographer.as_mut(),
        Some(ManagerSlot::Curator) => document.managers.curator.as_mut(),
        None => document.workers.get_mut(name),
    }
}

/// A name usable as one path segment of a canonical location.
fn segment(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// Moves every entry under `from/` to `to/`; refused when `to/` already holds anything.
fn move_dir(entries: &mut BTreeMap<String, Vec<u8>>, from: &str, to: &str) -> bool {
    if from == to {
        return true;
    }
    if entries.keys().any(|path| within(path, to)) {
        return false;
    }
    let inside: Vec<String> = entries
        .keys()
        .filter(|path| within(path, from))
        .cloned()
        .collect();
    for path in inside {
        if let Some(bytes) = entries.remove(&path) {
            entries.insert(format!("{to}{}", &path[from.len()..]), bytes);
        }
    }
    true
}

fn with_frontmatter(content: &str, name: &str, description: &str) -> String {
    if frontmatter_name(content).is_some() {
        return content.to_string();
    }
    let description = if description.is_empty() {
        name
    } else {
        description
    };
    format!("---\nname: {name}\ndescription: {description}\n---\n{content}")
}

/// The `.team` and folder forms carry one package as the same bytes whatever form it arrived
/// in: file and icon contents live in entries, role instructions in `managers/<slot>.md` or
/// `workers/<name>.md`, skills under `skills/<name>/` and Blueprints in `blueprints/<name>.yaml`.
pub fn externalized(package: &Package) -> Result<Package, String> {
    let mut package = package.clone();
    let mut entries = std::mem::take(&mut package.entries);
    let mut vacated: BTreeSet<String> = BTreeSet::new();
    let document = &mut package.document;
    for file in document.files.iter_mut() {
        if let Some(content) = file.content.take() {
            entries.insert(file.path.clone(), content.into_bytes());
        }
    }
    // The icon travels by `icon:`; a listing that only gave it an inline home goes.
    let icon = document.icon.clone();
    document
        .files
        .retain(|file| icon.as_deref() != Some(file.path.as_str()) || file.description.is_some());

    let names: Vec<String> = document
        .roles()
        .into_iter()
        .map(|(_, name, _)| name.to_string())
        .collect();
    for name in names.iter().filter(|name| segment(name)) {
        let home = match ManagerSlot::parse(name) {
            Some(_) => format!("managers/{name}.md"),
            None => format!("workers/{name}.md"),
        };
        let Some(role) = role_mut(document, name) else {
            continue;
        };
        if role.instructions_file.as_deref() == Some(home.as_str()) {
            continue;
        }
        let bytes = match (&role.instructions, &role.instructions_file) {
            (Some(text), _) => text.clone().into_bytes(),
            (None, Some(path)) => match entries.get(path) {
                Some(bytes) => bytes.clone(),
                None => continue,
            },
            (None, None) => continue,
        };
        if entries.contains_key(&home) {
            continue;
        }
        if let Some(previous) = role.instructions_file.take() {
            vacated.insert(previous);
        }
        role.instructions = None;
        role.instructions_file = Some(home.clone());
        entries.insert(home, bytes);
    }

    for skill in document.skills.iter_mut() {
        match skill {
            PackageSkill::Inline(inline) => {
                let dir = format!("skills/{}", inline.name);
                if !segment(&inline.name) || entries.keys().any(|path| within(path, &dir)) {
                    continue;
                }
                let manifest = with_frontmatter(&inline.content, &inline.name, &inline.description);
                entries.insert(format!("{dir}/SKILL.md"), manifest.into_bytes());
                for file in &inline.files {
                    entries.insert(
                        format!("{dir}/{}", file.path),
                        file.content.clone().into_bytes(),
                    );
                }
                *skill = PackageSkill::Path { path: dir };
            }
            PackageSkill::Path { path } => {
                let name = entries
                    .get(&format!("{path}/SKILL.md"))
                    .and_then(|data| std::str::from_utf8(data).ok())
                    .and_then(frontmatter_name);
                let Some(name) = name.filter(|name| segment(name)) else {
                    continue;
                };
                let dir = format!("skills/{name}");
                if move_dir(&mut entries, path, &dir) {
                    *path = dir;
                }
            }
        }
    }

    for automation in document.automation.iter_mut() {
        let (name, description, bytes, previous) = match automation {
            PackageAutomation::Inline {
                name,
                description,
                content,
            } => (
                name.clone(),
                description.clone(),
                content.clone().into_bytes(),
                None,
            ),
            PackageAutomation::Path { path, description } => {
                let Some(bytes) = entries.get(path.as_str()).cloned() else {
                    continue;
                };
                let name = std::str::from_utf8(&bytes)
                    .ok()
                    .and_then(|text| field(text, "name"))
                    .unwrap_or_default();
                (name, description.clone(), bytes, Some(path.clone()))
            }
        };
        let home = format!("blueprints/{name}.yaml");
        if !segment(&name) || previous.as_deref() == Some(home.as_str()) {
            continue;
        }
        if entries.contains_key(&home) {
            continue;
        }
        if let Some(previous) = previous {
            vacated.insert(previous);
        }
        entries.insert(home.clone(), bytes);
        *automation = PackageAutomation::Path {
            path: home,
            description,
        };
    }

    // A vacated path stays only while something else still names it.
    let named: BTreeSet<&str> = document
        .files
        .iter()
        .map(|file| file.path.as_str())
        .chain(document.icon.as_deref())
        .chain(
            document
                .roles()
                .into_iter()
                .filter_map(|(_, _, role)| role.instructions_file.as_deref()),
        )
        .chain(document.automation.iter().filter_map(|item| match item {
            PackageAutomation::Path { path, .. } => Some(path.as_str()),
            PackageAutomation::Inline { .. } => None,
        }))
        .collect();
    for path in vacated {
        if !named.contains(path.as_str()) {
            entries.remove(&path);
        }
    }
    package.entries = entries;
    Ok(package)
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
        if let Some(role) = role_mut(&mut package.document, &name) {
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

/// The package a format carries: the archive forms take the canonical externalized layout.
pub fn canonical_for(package: Package, format: PackageFormat) -> Result<Package, String> {
    match format {
        PackageFormat::Yaml => Ok(package),
        PackageFormat::Team | PackageFormat::Folder => externalized(&package),
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
