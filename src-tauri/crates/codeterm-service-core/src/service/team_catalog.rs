use std::collections::BTreeMap;
use std::sync::OnceLock;

use codeterm_team_package::contracts::{TemplateProvenance, TemplateRef, TemplateSummary};
use codeterm_team_package::{builtin, CheckOptions, Package};

use super::team_channels::{TeamChannelStore, OFFICIAL};
use super::team_exchange::library::TemplateLibrary;

pub fn bundled() -> &'static [(String, Package)] {
    static PACKAGES: OnceLock<Vec<(String, Package)>> = OnceLock::new();
    PACKAGES.get_or_init(|| {
        builtin::names()
            .into_iter()
            .filter_map(|folder| {
                let checked = builtin::check(&folder, &CheckOptions::default())?;
                let package = checked.package.filter(|_| checked.report.valid)?;
                super::team_template_start::host_policy(&package).ok()?;
                Some((folder, package))
            })
            .collect()
    })
}

#[derive(Clone)]
pub struct CatalogEntry {
    pub package: Package,
    pub provenance: TemplateProvenance,
    pub channel: Option<String>,
    pub bundled_version: Option<String>,
}

impl CatalogEntry {
    pub fn summary(&self, library: &TemplateLibrary) -> TemplateSummary {
        let mut summary = match self.provenance {
            TemplateProvenance::Saved | TemplateProvenance::Imported => {
                library.summary(&self.package)
            }
            _ => TemplateSummary::from_package(&self.package, self.provenance),
        };
        summary.channel = self.channel.clone();
        summary.bundled_version = self.bundled_version.clone();
        if matches!(self.provenance, TemplateProvenance::Channel) {
            summary.verified =
                super::team_template_start::ConsentLedger::global().accepted(&self.package);
        }
        summary
    }
}

pub fn entries(store: &TeamChannelStore, library: &TemplateLibrary) -> Vec<CatalogEntry> {
    layer(store.packages(), library)
}

fn layer(
    channels: Vec<(super::team_channels::TeamChannelRow, Package)>,
    library: &TemplateLibrary,
) -> Vec<CatalogEntry> {
    let mut entries: BTreeMap<String, CatalogEntry> = bundled()
        .iter()
        .map(|(_, package)| {
            (
                package.document.name.clone(),
                CatalogEntry {
                    package: package.clone(),
                    provenance: TemplateProvenance::Builtin,
                    channel: None,
                    bundled_version: None,
                },
            )
        })
        .collect();
    for (row, package) in channels {
        let name = &package.document.name;
        if row.name == OFFICIAL {
            let previous = entries.get(name);
            if let Some(previous) = previous {
                let versions = (
                    semver::Version::parse(&previous.package.document.version),
                    semver::Version::parse(&package.document.version),
                );
                if !matches!(versions, (Ok(old), Ok(new)) if new.cmp_precedence(&old).is_gt()) {
                    continue;
                }
            }
            let bundled_version = previous.map(|entry| entry.package.document.version.clone());
            entries.insert(
                name.clone(),
                CatalogEntry {
                    package,
                    provenance: TemplateProvenance::Builtin,
                    channel: Some(row.name),
                    bundled_version,
                },
            );
        } else if !name.starts_with("dev.domios/") {
            entries.entry(name.clone()).or_insert_with(|| CatalogEntry {
                package,
                provenance: TemplateProvenance::Channel,
                channel: Some(row.name),
                bundled_version: None,
            });
        }
    }
    for summary in library.list() {
        if summary.name.starts_with("dev.domios/") {
            continue;
        }
        if let Some((package, provenance)) = library.load(&summary.name, Some(&summary.version)) {
            entries.insert(
                summary.name,
                CatalogEntry {
                    package,
                    provenance,
                    channel: None,
                    bundled_version: None,
                },
            );
        }
    }
    let mut entries: Vec<_> = entries.into_values().collect();
    entries.sort_by_key(|entry| match entry.provenance {
        TemplateProvenance::Builtin => 0,
        TemplateProvenance::Channel => 1,
        _ => 2,
    });
    entries
}

pub fn resolve_in(
    store: &TeamChannelStore,
    library: &TemplateLibrary,
    template: &TemplateRef,
) -> Option<CatalogEntry> {
    let name = bundled()
        .iter()
        .find(|(folder, _)| *folder == template.name)
        .map(|(_, package)| package.document.name.as_str())
        .unwrap_or(&template.name);
    let matches = |package: &Package| {
        package.document.name == name
            && template
                .version
                .as_ref()
                .is_none_or(|version| *version == package.document.version)
    };
    if let Some(entry) = entries(store, library)
        .into_iter()
        .find(|entry| matches(&entry.package))
    {
        return Some(entry);
    }
    if let Some((_, package)) = bundled().iter().find(|(_, package)| matches(package)) {
        return Some(CatalogEntry {
            package: package.clone(),
            provenance: TemplateProvenance::Builtin,
            channel: None,
            bundled_version: None,
        });
    }
    if name.starts_with("dev.domios/") {
        return None;
    }
    let (package, provenance) = library.load(name, template.version.as_deref())?;
    Some(CatalogEntry {
        package,
        provenance,
        channel: None,
        bundled_version: None,
    })
}

pub fn summary(
    library: &TemplateLibrary,
    package: &Package,
    provenance: TemplateProvenance,
) -> TemplateSummary {
    resolve_in(
        TeamChannelStore::global(),
        library,
        &TemplateRef {
            name: package.document.name.clone(),
            version: Some(package.document.version.clone()),
        },
    )
    .map(|entry| entry.summary(library))
    .unwrap_or_else(|| TemplateSummary::from_package(package, provenance))
}

#[cfg(test)]
mod tests;
