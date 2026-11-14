use codeterm_team_package::contracts::{TemplateProvenance, TemplateRef, TemplateStartRefusal};
use codeterm_team_package::{
    check_entries, CheckOptions, Checked, Package, PackageDiagnostic, PackageFormat, RoleKind,
    RoleStart, CURATOR,
};
use codeterm_types::agent::role_kind::builtins::{is_taskless_worker_kind, GENERAL_AGENT, WATCHER};
use codeterm_types::team::role_registration::PinnedPackage;

use crate::service::builtin_roles::DEFAULT_PACKAGE;

/// A checked package ready to start; its warnings travel with the start result.
#[derive(Debug, Clone)]
pub struct ResolvedTemplate {
    pub package: Package,
    pub provenance: TemplateProvenance,
    pub warnings: Vec<PackageDiagnostic>,
}

impl ResolvedTemplate {
    /// Any of the three forms, read by the one parser: errors refuse, warnings are kept.
    pub fn from_checked(
        checked: Checked,
        provenance: TemplateProvenance,
    ) -> Result<Self, TemplateStartRefusal> {
        let Checked { report, package } = checked;
        match package.filter(|_| report.valid) {
            Some(package) => {
                host_policy(&package)?;
                Ok(Self {
                    package,
                    provenance,
                    warnings: report.warnings().cloned().collect(),
                })
            }
            None => Err(TemplateStartRefusal::InvalidPackage {
                diagnostics: report.errors().cloned().collect(),
            }),
        }
    }

    pub fn identity(&self) -> PinnedPackage {
        PinnedPackage {
            name: self.package.document.name.clone(),
            version: self.package.document.version.clone(),
        }
    }

    pub fn requires_harnesses(&self) -> &[String] {
        self.package
            .document
            .requires
            .as_ref()
            .map(|requires| requires.harnesses.as_slice())
            .unwrap_or_default()
    }
}

/// What Domios refuses in a valid package: its own host-owned names as roles, and a schedule
/// on any role but a curator manager, the only role it runs scheduled rounds for.
pub fn host_policy(package: &Package) -> Result<(), TemplateStartRefusal> {
    refuse_host_owned_roles(package)?;
    for (name, role) in package.document.roles() {
        let scheduled = matches!(role.start, Some(RoleStart::Schedule { .. }));
        if scheduled && !(name == CURATOR && role.kind == RoleKind::Manager) {
            return Err(TemplateStartRefusal::ScheduleUnsupportedRole {
                role: name.to_string(),
            });
        }
    }
    Ok(())
}

/// Domios owns `watcher` and `general_agent`, and its worker kinds are never managers: a package
/// declaring either is refused here, before a Team or a tab exists.
pub fn refuse_host_owned_roles(package: &Package) -> Result<(), TemplateStartRefusal> {
    match package.document.roles().find(|(name, role)| {
        *name == WATCHER
            || *name == GENERAL_AGENT
            || (role.kind == RoleKind::Manager && is_taskless_worker_kind(name))
    }) {
        Some((name, _)) => Err(TemplateStartRefusal::HostOwnedRole {
            role: name.to_string(),
        }),
        None => Ok(()),
    }
}

pub fn builtin_templates() -> impl Iterator<Item = ResolvedTemplate> {
    crate::service::team_catalog::entries(
        crate::service::team_channels::TeamChannelStore::global(),
        crate::service::team_exchange::library::TemplateLibrary::global(),
    )
    .into_iter()
    .filter(|entry| entry.provenance == TemplateProvenance::Builtin)
    .map(|entry| ResolvedTemplate {
        package: entry.package,
        provenance: entry.provenance,
        warnings: Vec::new(),
    })
}

/// The one package lookup start, preview and re-run use: an absent template is the builtin
/// default; a name resolves through the exchange store (builtins first, then My templates)
/// and is checked again by the one parser so its warnings travel with the start.
pub fn resolve(template: Option<&TemplateRef>) -> Result<ResolvedTemplate, TemplateStartRefusal> {
    let default_ref = TemplateRef {
        name: DEFAULT_PACKAGE.to_string(),
        version: None,
    };
    let requested = template.unwrap_or(&default_ref);
    let (package, provenance) =
        crate::service::team_exchange::resolve(requested).ok_or_else(|| {
            TemplateStartRefusal::NotFound {
                template: requested.clone(),
            }
        })?;
    ResolvedTemplate::from_checked(
        check_entries(
            PackageFormat::Folder,
            package.archive_entries(),
            Vec::new(),
            &CheckOptions::default(),
        ),
        provenance,
    )
}
