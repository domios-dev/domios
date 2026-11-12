//! My templates: checked packages saved under the data directory as deterministic `.team`
//! archives, one per `name@version`. Saving never touches a Team's registrations, so an
//! active Team keeps the version it was started from.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use codeterm_team_package::contracts::{
    declared_asks, TemplateDiffItem, TemplateProvenance, TemplateRef, TemplateSummary,
    TemplateUpdateDiff,
};
use codeterm_team_package::{check_archive, payload_items, CheckOptions, Package};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

const ARCHIVE: &str = "team";
const RECORD: &str = "json";
/// Builtin packages own this namespace; My templates never shadow them.
pub const BUILTIN_OWNER: &str = "dev.domios";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Record {
    provenance: TemplateProvenance,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    received_from: Option<String>,
    /// A start of this version passed the owner's consent here.
    #[serde(default)]
    verified: bool,
}

impl Record {
    fn new(provenance: TemplateProvenance) -> Self {
        Self {
            provenance,
            received_from: None,
            verified: false,
        }
    }
}

/// What a save changed.
#[derive(Debug, Clone, PartialEq)]
pub enum Saved {
    Created {
        update: Option<TemplateUpdateDiff>,
    },
    /// The same `name@version` with the same bytes is already saved.
    Unchanged,
}

pub struct TemplateLibrary {
    root: PathBuf,
    writes: Mutex<()>,
}

/// `(major, minor, patch, release, text)`: a release sorts after its pre-releases.
fn version_key(version: &str) -> (u64, u64, u64, bool, String) {
    let core = version.split('+').next().unwrap_or_default();
    let (numbers, pre) = match core.split_once('-') {
        Some((numbers, pre)) => (numbers, Some(pre)),
        None => (core, None),
    };
    let mut parts = numbers
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0));
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        pre.is_none(),
        pre.unwrap_or_default().to_string(),
    )
}

/// Role changes, payload changes and new asks between two versions of one template.
pub fn update_diff(from: &Package, to: &Package) -> TemplateUpdateDiff {
    let roles = |package: &Package| {
        package
            .document
            .roles()
            .into_iter()
            .map(|(_, name, role)| (name.to_string(), role.clone()))
            .collect::<Vec<_>>()
    };
    let (before, after) = (roles(from), roles(to));
    let named = |list: &[(String, codeterm_team_package::TeamRole)], name: &str| {
        list.iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, role)| role.clone())
    };
    let mut diff = TemplateUpdateDiff {
        name: to.document.name.clone(),
        from_version: from.document.version.clone(),
        to_version: to.document.version.clone(),
        ..Default::default()
    };
    for (name, role) in &after {
        match named(&before, name) {
            None => diff.roles_added.push(name.clone()),
            Some(previous)
                if previous != *role
                    || from.role_instructions(name) != to.role_instructions(name) =>
            {
                diff.roles_changed.push(name.clone())
            }
            Some(_) => {}
        }
    }
    for (name, _) in &before {
        if named(&after, name).is_none() {
            diff.roles_removed.push(name.clone());
        }
    }
    let items = |package: &Package| {
        payload_items(package)
            .into_iter()
            .map(|item| (item.id, item.label))
            .collect::<BTreeMap<_, _>>()
    };
    let only_in = |kept: &BTreeMap<String, String>, other: &BTreeMap<String, String>| {
        kept.iter()
            .filter(|(id, _)| !other.contains_key(*id))
            .map(|(id, label)| TemplateDiffItem {
                id: id.clone(),
                label: label.clone(),
            })
            .collect()
    };
    let (old_items, new_items) = (items(from), items(to));
    diff.payload_added = only_in(&new_items, &old_items);
    diff.payload_removed = only_in(&old_items, &new_items);
    let old_asks = declared_asks(from);
    diff.asks_added = declared_asks(to)
        .into_iter()
        .filter(|ask| !old_asks.contains(ask))
        .collect();
    diff
}

impl TemplateLibrary {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            writes: Mutex::new(()),
        }
    }

    /// The instance library under the data directory.
    pub fn global() -> &'static Self {
        static LIBRARY: OnceLock<TemplateLibrary> = OnceLock::new();
        LIBRARY.get_or_init(|| Self::new(crate::paths::data_dir().join("team-templates")))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn dir(&self, name: &str) -> Result<PathBuf, String> {
        if !codeterm_team_package::is_package_name(name) {
            return Err(format!("`{name}` is not a package name"));
        }
        let (owner, slug) = name.split_once('/').unwrap_or_default();
        Ok(self.root.join(owner).join(slug))
    }

    fn file(&self, name: &str, version: &str, extension: &str) -> Result<PathBuf, String> {
        if !codeterm_team_package::is_semver(version) {
            return Err(format!("`{version}` is not a version"));
        }
        Ok(self.dir(name)?.join(format!("{version}.{extension}")))
    }

    /// Saved versions of `name`, oldest first.
    pub fn versions(&self, name: &str) -> Vec<String> {
        let Ok(dir) = self.dir(name) else {
            return Vec::new();
        };
        let Ok(read) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut versions: Vec<String> = read
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name.strip_suffix(&format!(".{ARCHIVE}"))
                    .map(str::to_string)
            })
            .filter(|version| codeterm_team_package::is_semver(version))
            .collect();
        versions.sort_by_key(|version| version_key(version));
        versions
    }

    /// One saved version, or the newest when `version` is absent; re-checked on every read.
    pub fn load(&self, name: &str, version: Option<&str>) -> Option<(Package, TemplateProvenance)> {
        let version = match version {
            Some(version) => version.to_string(),
            None => self.versions(name).pop()?,
        };
        let archive = self.file(name, &version, ARCHIVE).ok()?;
        codeterm_util::safe_tree::refuse_linked_ancestors(&archive).ok()?;
        let bytes = std::fs::read(&archive).ok()?;
        let package = check_archive(&bytes, &CheckOptions::domios()).package?;
        Some((package, self.record(name, &version).provenance))
    }

    /// The saved record of one version; an unreadable record is an unverified import.
    fn record(&self, name: &str, version: &str) -> Record {
        self.file(name, version, RECORD)
            .ok()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str::<Record>(&text).ok())
            .unwrap_or_else(|| Record::new(TemplateProvenance::Imported))
    }

    fn write_record(&self, name: &str, version: &str, record: &Record) -> Result<(), String> {
        let text = serde_json::to_string_pretty(record).map_err(|error| error.to_string())?;
        codeterm_util::atomic_write::write_atomic(&self.file(name, version, RECORD)?, text)
            .map_err(|error| error.to_string())
    }

    /// Updates the record of a saved version; an absent version is not created.
    fn amend(
        &self,
        name: &str,
        version: &str,
        change: impl FnOnce(&mut Record),
    ) -> Result<(), String> {
        let _write = self.writes.lock();
        if !self.file(name, version, ARCHIVE)?.is_file() {
            return Err(format!("`{name}@{version}` is not in My templates"));
        }
        let mut record = self.record(name, version);
        change(&mut record);
        self.write_record(name, version, &record)
    }

    /// Records the friend a saved version arrived from.
    pub fn mark_received(&self, name: &str, version: &str, from: &str) -> Result<(), String> {
        self.amend(name, version, |record| {
            record.received_from = Some(from.to_string())
        })
    }

    /// Records that a start of this version passed the owner's consent.
    pub fn mark_verified(&self, name: &str, version: &str) -> Result<(), String> {
        self.amend(name, version, |record| record.verified = true)
    }

    /// The summary of a saved package with what its record knows about it.
    pub fn summary(&self, package: &Package) -> TemplateSummary {
        let document = &package.document;
        let record = self.record(&document.name, &document.version);
        let mut summary = TemplateSummary::from_package(package, record.provenance);
        summary.received_from = record.received_from;
        summary.verified = record.verified;
        summary
    }

    /// The one summary of a resolved template, so list, preview and recall show the same trust.
    pub fn summarize(&self, package: &Package, provenance: TemplateProvenance) -> TemplateSummary {
        match provenance {
            TemplateProvenance::Builtin => TemplateSummary::from_package(package, provenance),
            _ => self.summary(package),
        }
    }

    /// The newest version of every saved template, by name.
    pub fn list(&self) -> Vec<TemplateSummary> {
        let mut names = Vec::new();
        for owner in std::fs::read_dir(&self.root)
            .into_iter()
            .flatten()
            .flatten()
        {
            for slug in std::fs::read_dir(owner.path())
                .into_iter()
                .flatten()
                .flatten()
            {
                names.push(format!(
                    "{}/{}",
                    owner.file_name().to_string_lossy(),
                    slug.file_name().to_string_lossy()
                ));
            }
        }
        names.sort();
        names
            .into_iter()
            .filter_map(|name| self.load(&name, None))
            .map(|(package, _)| self.summary(&package))
            .collect()
    }

    /// Saves a checked package. Another version of the name is kept and reported as an update;
    /// the same version with other content is refused so a pinned Team never changes meaning.
    pub fn save(&self, package: &Package, provenance: TemplateProvenance) -> Result<Saved, String> {
        let name = package.document.name.as_str();
        let version = package.document.version.as_str();
        if name.split('/').next() == Some(BUILTIN_OWNER) {
            return Err(format!(
                "`{name}` uses the builtin `{BUILTIN_OWNER}/` namespace; save it under your own name"
            ));
        }
        let bytes = package.to_archive()?;
        let archive = self.file(name, version, ARCHIVE)?;
        let _write = self.writes.lock();
        if let Ok(existing) = std::fs::read(&archive) {
            if existing == bytes {
                return Ok(Saved::Unchanged);
            }
            return Err(format!(
                "`{name}@{version}` is already saved with other content; bump the version to save a change"
            ));
        }
        let update = self
            .load(name, None)
            .map(|(previous, _)| update_diff(&previous, package));
        let dir = self.dir(name)?;
        std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        codeterm_util::safe_tree::refuse_linked_ancestors(&dir)?;
        self.write_record(name, version, &Record::new(provenance))?;
        codeterm_util::atomic_write::write_atomic(&archive, &bytes)
            .map_err(|error| error.to_string())?;
        Ok(Saved::Created { update })
    }

    /// Removes one version, or every version when `version` is absent.
    pub fn delete(&self, template: &TemplateRef) -> Result<Vec<String>, String> {
        let _write = self.writes.lock();
        let versions = match &template.version {
            Some(version) => vec![version.clone()],
            None => self.versions(&template.name),
        };
        let mut removed = Vec::new();
        for version in versions {
            let archive = self.file(&template.name, &version, ARCHIVE)?;
            if !archive.is_file() {
                continue;
            }
            codeterm_util::safe_tree::refuse_linked_ancestors(&archive)?;
            std::fs::remove_file(&archive).map_err(|error| error.to_string())?;
            let _ = std::fs::remove_file(self.file(&template.name, &version, RECORD)?);
            removed.push(format!("{}@{version}", template.name));
        }
        if removed.is_empty() {
            return Err(format!("`{}` is not in My templates", template.name));
        }
        let dir = self.dir(&template.name)?;
        if std::fs::read_dir(&dir).is_ok_and(|mut entries| entries.next().is_none()) {
            let _ = std::fs::remove_dir(&dir);
        }
        Ok(removed)
    }
}

#[cfg(test)]
pub(super) fn sorted_versions(mut versions: Vec<&str>) -> Vec<&str> {
    versions.sort_by_key(|version| version_key(version));
    versions
}
