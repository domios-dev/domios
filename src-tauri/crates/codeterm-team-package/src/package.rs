use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::archive::{self, entry_error};
use crate::diagnostics::{DiagnosticCode, PackageCheckReport, PackageDiagnostic, Sink};
use crate::document::{PackageAutomation, PackageSkill, TeamDocument, TeamRoleGroup};
use crate::image::{check_icon, IconKind};
use crate::paths::{within, ROOT_DOCUMENT};
use crate::{parse, serialize, validate, yaml, CheckOptions, ExtensionValue, OrderedMap};

/// The three interchangeable forms of one package, and the export `--format` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum PackageFormat {
    /// `name.team.yaml`: one file with everything inline.
    Yaml,
    /// `name.team`: a ZIP with `team.yaml` at its root plus referenced files.
    Team,
    /// A folder with the same layout as the ZIP.
    Folder,
}

/// A checked package: its document plus the files the archive or folder form carries.
#[derive(Debug, Clone, PartialEq)]
pub struct Package {
    /// The leading `#` comment block of `team.yaml`, re-emitted verbatim.
    pub header: Option<String>,
    pub document: TeamDocument,
    pub extensions: OrderedMap<ExtensionValue>,
    /// Referenced files other than `team.yaml`, by package-relative path.
    pub entries: BTreeMap<String, Vec<u8>>,
}

/// The validator's report plus the package when it has no errors.
#[derive(Debug, Clone)]
pub struct Checked {
    pub report: PackageCheckReport,
    pub package: Option<Package>,
}

impl Package {
    /// The canonical `team.yaml` text.
    pub fn to_yaml(&self) -> String {
        serialize::to_yaml(self)
    }

    /// Archive and folder entries in their deterministic order: `team.yaml` first, then by path.
    pub fn archive_entries(&self) -> Vec<(String, Vec<u8>)> {
        let mut entries = vec![(ROOT_DOCUMENT.to_string(), self.to_yaml().into_bytes())];
        entries.extend(
            self.entries
                .iter()
                .map(|(path, data)| (path.clone(), data.clone())),
        );
        entries
    }

    /// The `.team` ZIP bytes.
    pub fn to_archive(&self) -> Result<Vec<u8>, String> {
        archive::write_zip(&self.archive_entries())
    }

    /// Writes the folder form under `dir`.
    pub fn write_folder(&self, dir: &Path) -> std::io::Result<()> {
        archive::write_folder(dir, &self.archive_entries())
    }

    /// The bytes of a package file, inline `files` content first.
    pub fn file_bytes(&self, path: &str) -> Option<&[u8]> {
        self.document
            .files
            .iter()
            .find(|file| file.path == path)
            .and_then(|file| file.content.as_deref())
            .map(str::as_bytes)
            .or_else(|| self.entries.get(path).map(Vec::as_slice))
    }

    /// A role's own instructions: inline text, or its `instructions_file` content.
    pub fn role_instructions(&self, name: &str) -> Option<&str> {
        let role = self.document.role(name)?;
        if let Some(text) = role.instructions.as_deref() {
            return Some(text);
        }
        let path = role.instructions_file.as_deref()?;
        self.file_bytes(path)
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
    }

    /// The name roles use for a skill: the inline name, or the SKILL.md frontmatter `name`.
    pub fn skill_name(&self, skill: &PackageSkill) -> Option<String> {
        match skill {
            PackageSkill::Inline(inline) => Some(inline.name.clone()),
            PackageSkill::Path { path } => self
                .entries
                .get(&format!("{path}/SKILL.md"))
                .and_then(|data| std::str::from_utf8(data).ok())
                .and_then(frontmatter_name),
        }
    }

    /// The icon bytes and kind, when the package declares one.
    pub fn icon(&self) -> Option<(IconKind, &[u8])> {
        let path = self.document.icon.as_deref()?;
        let bytes = self.file_bytes(path)?;
        check_icon(path, bytes).ok().map(|kind| (kind, bytes))
    }
}

/// Validates the one-file form.
pub fn check_yaml(text: &str, options: &CheckOptions) -> Checked {
    check_entries(
        PackageFormat::Yaml,
        vec![(ROOT_DOCUMENT.to_string(), text.as_bytes().to_vec())],
        Vec::new(),
        options,
    )
}

/// Validates the `.team` ZIP form.
pub fn check_archive(bytes: &[u8], options: &CheckOptions) -> Checked {
    let mut diagnostics = Vec::new();
    match archive::read_zip(bytes, &options.limits, &mut diagnostics) {
        Some(entries) => check_entries(PackageFormat::Team, entries, diagnostics, options),
        None => refused(PackageFormat::Team, diagnostics),
    }
}

/// Validates the folder form.
pub fn check_folder(dir: &Path, options: &CheckOptions) -> Checked {
    let mut diagnostics = Vec::new();
    match archive::read_folder(dir, &options.limits, &mut diagnostics) {
        Some(entries) => check_entries(PackageFormat::Folder, entries, diagnostics, options),
        None => refused(PackageFormat::Folder, diagnostics),
    }
}

/// The form a path names: a folder, a `.team` archive, or otherwise the one-file form.
pub fn format_of(path: &Path) -> PackageFormat {
    if path.is_dir() {
        PackageFormat::Folder
    } else if path
        .extension()
        .is_some_and(|extension| extension == "team")
    {
        PackageFormat::Team
    } else {
        PackageFormat::Yaml
    }
}

/// Reads and validates a package from disk in whichever form the path names.
pub fn check_path(path: &Path, options: &CheckOptions) -> Checked {
    let format = format_of(path);
    let limits = &options.limits;
    if format == PackageFormat::Folder {
        return check_folder(path, options);
    }
    let unreadable = |message: String| {
        refused(
            format,
            vec![entry_error("", DiagnosticCode::Unreadable, message)],
        )
    };
    let size_limit = match format {
        PackageFormat::Team => limits.max_total_bytes,
        _ => limits.max_yaml_bytes,
    };
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.len() > size_limit as u64 => {
            return refused(
                format,
                vec![entry_error(
                    "",
                    DiagnosticCode::TooLarge,
                    format!("the package exceeds {size_limit} bytes"),
                )],
            )
        }
        Ok(_) => {}
        Err(error) => return unreadable(format!("{}: {error}", path.display())),
    }
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => return unreadable(format!("{}: {error}", path.display())),
    };
    match format {
        PackageFormat::Team => check_archive(&bytes, options),
        _ => match String::from_utf8(bytes) {
            Ok(text) => check_yaml(&text, options),
            Err(_) => refused(
                format,
                vec![entry_error(
                    ROOT_DOCUMENT,
                    DiagnosticCode::NotUtf8,
                    "team.yaml must be UTF-8",
                )],
            ),
        },
    }
}

fn refused(format: PackageFormat, diagnostics: Vec<PackageDiagnostic>) -> Checked {
    Checked {
        report: PackageCheckReport {
            format,
            valid: false,
            name: None,
            version: None,
            diagnostics,
        },
        package: None,
    }
}

/// The shared core for every form: entry rules, document rules, then references.
pub fn check_entries(
    format: PackageFormat,
    entries: Vec<(String, Vec<u8>)>,
    mut diagnostics: Vec<PackageDiagnostic>,
    options: &CheckOptions,
) -> Checked {
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut root = None;
    for (path, data) in entries {
        if path == ROOT_DOCUMENT {
            if root.replace(data).is_some() {
                diagnostics.push(entry_error(
                    &path,
                    DiagnosticCode::DuplicateEntry,
                    "the archive holds two team.yaml entries",
                ));
            }
        } else if files.contains_key(&path) {
            diagnostics.push(entry_error(
                &path,
                DiagnosticCode::DuplicateEntry,
                "the archive holds this path twice",
            ));
        } else {
            files.insert(path, data);
        }
    }
    let Some(root) = root else {
        diagnostics.push(entry_error(
            ROOT_DOCUMENT,
            DiagnosticCode::MissingRoot,
            "a package has exactly one root team.yaml",
        ));
        return refused(format, diagnostics);
    };
    let Ok(text) = String::from_utf8(root) else {
        diagnostics.push(entry_error(
            ROOT_DOCUMENT,
            DiagnosticCode::NotUtf8,
            "team.yaml must be UTF-8",
        ));
        return refused(format, diagnostics);
    };
    let mut sink = Sink::new(ROOT_DOCUMENT);
    let mut package = None;
    if let Some(loaded) = yaml::load(&text, &options.limits, &mut sink) {
        if let Some((document, extensions)) = parse::document(&loaded.root, &mut sink) {
            validate::document(&document, &loaded.locations, options, &mut sink);
            references(
                &document,
                &files,
                &loaded.locations,
                &mut sink,
                &mut diagnostics,
            );
            package = Some(Package {
                header: loaded.header,
                document,
                extensions,
                entries: files,
            });
        }
    }
    diagnostics.extend(sink.diagnostics);
    let valid = !diagnostics.iter().any(PackageDiagnostic::is_error);
    let (name, version) = package
        .as_ref()
        .map(|package| {
            (
                Some(package.document.name.clone()),
                Some(package.document.version.clone()),
            )
        })
        .unwrap_or_default();
    Checked {
        report: PackageCheckReport {
            format,
            valid,
            name,
            version,
            diagnostics,
        },
        package: package.filter(|_| valid),
    }
}

struct Resolver<'a> {
    files: &'a BTreeMap<String, Vec<u8>>,
    used: BTreeSet<&'a str>,
    locations: &'a yaml::Locations,
}

impl<'a> Resolver<'a> {
    fn require(&mut self, sink: &mut Sink, path: &str, at: &str, text: bool) {
        match self.files.get_key_value(path) {
            Some((key, data)) => {
                self.used.insert(key.as_str());
                if text && std::str::from_utf8(data).is_err() {
                    sink.error(
                        DiagnosticCode::NotUtf8,
                        at,
                        self.locations.at(at),
                        format!("`{path}` must be UTF-8 text"),
                    );
                }
            }
            None => sink.error(
                DiagnosticCode::MissingFile,
                at,
                self.locations.at(at),
                format!("`{path}` is referenced but not in the package"),
            ),
        }
    }

    fn mark_within(&mut self, dir: &str) {
        let files = self.files;
        for key in files.keys().filter(|key| within(key, dir)) {
            self.used.insert(key.as_str());
        }
    }
}

/// Resolves every file reference against inline content and entries; flags unreferenced entries.
fn references(
    document: &TeamDocument,
    files: &BTreeMap<String, Vec<u8>>,
    locations: &yaml::Locations,
    sink: &mut Sink,
    diagnostics: &mut Vec<PackageDiagnostic>,
) {
    let mut resolver = Resolver {
        files,
        used: BTreeSet::new(),
        locations,
    };
    for (index, file) in document.files.iter().enumerate() {
        let at = format!("files[{index}]");
        match &file.content {
            Some(_) if files.contains_key(&file.path) => sink.error(
                DiagnosticCode::FileConflict,
                &at,
                locations.at(&at),
                format!("`{}` is both inline and a package entry", file.path),
            ),
            Some(_) => {}
            None => resolver.require(sink, &file.path, &format!("{at}.path"), false),
        }
    }
    for (group, name, role) in document.roles() {
        if let Some(path) = &role.instructions_file {
            let section = match group {
                TeamRoleGroup::Manager => "managers",
                TeamRoleGroup::Worker => "workers",
            };
            resolver.require(
                sink,
                path,
                &format!("{section}.{name}.instructions_file"),
                true,
            );
        }
    }
    let mut skill_names = BTreeSet::new();
    for (index, skill) in document.skills.iter().enumerate() {
        let at = format!("skills[{index}]");
        let name = match skill {
            PackageSkill::Inline(inline) => Some(inline.name.clone()),
            PackageSkill::Path { path } => {
                let manifest = format!("{path}/SKILL.md");
                resolver.require(sink, &manifest, &format!("{at}.path"), true);
                resolver.mark_within(path);
                let name = files
                    .get(&manifest)
                    .and_then(|data| std::str::from_utf8(data).ok())
                    .map(frontmatter_name);
                if name == Some(None) {
                    sink.error(
                        DiagnosticCode::MissingField,
                        &format!("{at}.path"),
                        locations.at(&at),
                        format!("`{manifest}` needs a frontmatter `name`"),
                    );
                }
                name.flatten()
            }
        };
        if let Some(name) = name {
            if !skill_names.insert(name.clone()) {
                sink.error(
                    DiagnosticCode::DuplicateSkill,
                    &at,
                    locations.at(&at),
                    format!("skill `{name}` is declared twice"),
                );
            }
        }
    }
    for (group, name, role) in document.roles() {
        let section = match group {
            TeamRoleGroup::Manager => "managers",
            TeamRoleGroup::Worker => "workers",
        };
        for (index, skill) in role.skills.iter().enumerate() {
            if !skill_names.contains(skill) {
                let at = format!("{section}.{name}.skills[{index}]");
                sink.warning(
                    DiagnosticCode::UnresolvedSkill,
                    &at,
                    locations.at(&at),
                    format!("skill `{skill}` is not in this package; the role starts without it until it is installed"),
                );
            }
        }
    }
    for (index, automation) in document.automation.iter().enumerate() {
        if let PackageAutomation::Path { path, .. } = automation {
            resolver.require(sink, path, &format!("automation[{index}].path"), true);
        }
    }
    if let Some(icon) = &document.icon {
        let inline = document
            .files
            .iter()
            .find(|file| &file.path == icon)
            .and_then(|file| file.content.as_deref());
        let bytes = match inline {
            Some(_) if icon.to_ascii_lowercase().ends_with(".png") => {
                sink.error(
                    DiagnosticCode::BinaryInline,
                    "icon",
                    locations.at("icon"),
                    "a PNG icon needs the `.team` or folder form; only an SVG can be inline",
                );
                None
            }
            Some(content) => Some(content.as_bytes()),
            None => match files.get(icon) {
                Some(data) => Some(data.as_slice()),
                None => {
                    sink.error(
                        DiagnosticCode::MissingFile,
                        "icon",
                        locations.at("icon"),
                        format!("icon `{icon}` is not in the package"),
                    );
                    None
                }
            },
        };
        if let Some(bytes) = bytes {
            if let Err((code, message)) = check_icon(icon, bytes) {
                sink.error(code, "icon", locations.at("icon"), message);
            }
        }
        if inline.is_none() {
            resolver.mark_within(icon);
        }
    }
    for (path, _) in files
        .iter()
        .filter(|(path, _)| !resolver.used.contains(path.as_str()))
    {
        diagnostics.push(entry_error(
            path,
            DiagnosticCode::UnreferencedEntry,
            "the package carries a file that team.yaml never references",
        ));
    }
}

/// The `name` from a SKILL.md YAML frontmatter block.
pub fn frontmatter_name(text: &str) -> Option<String> {
    let mut lines = text.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    lines
        .take_while(|line| line.trim_end() != "---")
        .find_map(|line| line.strip_prefix("name:"))
        .map(|value| value.trim().trim_matches(['"', '\'']).to_string())
        .filter(|name| !name.is_empty())
}
