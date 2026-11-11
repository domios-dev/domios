use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::document::{PackageAutomation, PackageSkill, TeamRole};
use crate::paths::within;
use crate::Package;

const SCRIPT_EXTENSIONS: &[&str] = &[
    "sh", "bash", "zsh", "fish", "ps1", "psm1", "bat", "cmd", "py", "js", "mjs", "cjs", "ts", "rb",
    "pl", "php", "lua", "exe", "dll", "so", "dylib", "jar",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PayloadKind {
    File,
    Memory,
    Skill,
    Automation,
}

/// One thing a package brings; the owner may opt out of it by `id` at start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplatePayloadItem {
    pub id: String,
    pub kind: PayloadKind,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub executable: bool,
    /// The roles whose `files` or `skills` name this item.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub used_by: Vec<String>,
    /// The first runnable path the item carries, shown beside its script ask.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script_path: Option<String>,
}

/// True for content a host would run: a shebang, a script or binary extension, or a `scripts/`/`bin/` folder.
pub fn is_executable(path: &str, content: &[u8]) -> bool {
    let extension = path
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, extension)| extension.to_ascii_lowercase());
    content.starts_with(b"#!")
        || extension.is_some_and(|extension| SCRIPT_EXTENSIONS.contains(&extension.as_str()))
        || path
            .split('/')
            .rev()
            .skip(1)
            .any(|component| component == "scripts" || component == "bin")
}

fn skill_script(package: &Package, skill: &PackageSkill) -> Option<String> {
    match skill {
        PackageSkill::Inline(inline) => inline
            .files
            .iter()
            .find(|file| is_executable(&file.path, file.content.as_bytes()))
            .map(|file| format!("{}/{}", inline.name, file.path)),
        PackageSkill::Path { path } => package
            .entries
            .iter()
            .filter(|(entry, _)| within(entry, path))
            .find(|(entry, data)| is_executable(entry, data))
            .map(|(entry, _)| entry.clone()),
    }
}

/// The roles that list `name` through `pick` (their files or their skills), in document order.
fn used_by(package: &Package, name: &str, pick: fn(&TeamRole) -> &[String]) -> Vec<String> {
    package
        .document
        .roles()
        .into_iter()
        .filter(|(_, _, role)| pick(role).iter().any(|entry| entry == name))
        .map(|(_, role, _)| role.to_string())
        .collect()
}

/// The payload list a preview shows, in document order with stable ids.
pub fn payload_items(package: &Package) -> Vec<TemplatePayloadItem> {
    let document = &package.document;
    let files = document.files.iter().map(|file| {
        let executable = package
            .file_bytes(&file.path)
            .is_some_and(|bytes| is_executable(&file.path, bytes));
        TemplatePayloadItem {
            id: format!("file:{}", file.path),
            kind: PayloadKind::File,
            label: file.path.clone(),
            description: file.description.clone(),
            executable,
            used_by: used_by(package, &file.path, |role| role.files.as_slice()),
            script_path: executable.then(|| file.path.clone()),
        }
    });
    let memory = document
        .memory
        .iter()
        .enumerate()
        .map(|(index, item)| TemplatePayloadItem {
            id: format!("memory:{index}"),
            kind: PayloadKind::Memory,
            label: item.title.clone(),
            description: None,
            executable: false,
            used_by: Vec::new(),
            script_path: None,
        });
    let skills = document.skills.iter().map(|skill| {
        let key = match skill {
            PackageSkill::Path { path } => path.clone(),
            PackageSkill::Inline(inline) => inline.name.clone(),
        };
        let label = package.skill_name(skill).unwrap_or_else(|| key.clone());
        let script_path = skill_script(package, skill);
        TemplatePayloadItem {
            id: format!("skill:{key}"),
            kind: PayloadKind::Skill,
            used_by: used_by(package, &label, |role| role.skills.as_slice()),
            label,
            description: match skill {
                PackageSkill::Inline(inline) => Some(inline.description.clone()),
                PackageSkill::Path { .. } => None,
            },
            executable: script_path.is_some(),
            script_path,
        }
    });
    let automation = document.automation.iter().map(|automation| {
        let (key, description) = match automation {
            PackageAutomation::Path { path, description } => (path.clone(), description.clone()),
            PackageAutomation::Inline {
                name, description, ..
            } => (name.clone(), description.clone()),
        };
        TemplatePayloadItem {
            id: format!("automation:{key}"),
            kind: PayloadKind::Automation,
            label: key,
            description,
            executable: false,
            used_by: Vec::new(),
            script_path: None,
        }
    });
    files
        .chain(memory)
        .chain(skills)
        .chain(automation)
        .collect()
}
