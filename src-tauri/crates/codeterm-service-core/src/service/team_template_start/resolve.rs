use std::sync::OnceLock;

use codeterm_team_package::contracts::{TemplateProvenance, TemplateRef, TemplateStartRefusal};
use codeterm_team_package::{
    builtin, check_entries, CheckOptions, Checked, Package, PackageDiagnostic, PackageFormat,
    RoleKind, RoleStart, CURATOR,
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

/// A builtin package and the folder it ships in.
struct Builtin {
    folder: String,
    template: ResolvedTemplate,
}

impl Builtin {
    fn answers(&self, template: &TemplateRef) -> bool {
        let document = &self.template.package.document;
        (template.name == document.name || template.name == self.folder)
            && template
                .version
                .as_deref()
                .is_none_or(|version| version == document.version)
    }
}

/// Every builtin package that passes the host check, checked once per process.
fn builtins() -> &'static [Builtin] {
    static BUILTINS: OnceLock<Vec<Builtin>> = OnceLock::new();
    BUILTINS.get_or_init(|| {
        builtin::names()
            .into_iter()
            .filter_map(|folder| {
                let checked = builtin::check(&folder, &CheckOptions::default())?;
                let template =
                    ResolvedTemplate::from_checked(checked, TemplateProvenance::Builtin).ok()?;
                Some(Builtin { folder, template })
            })
            .collect()
    })
}

pub fn builtin_templates() -> impl Iterator<Item = &'static ResolvedTemplate> {
    builtins().iter().map(|builtin| &builtin.template)
}

/// A builtin folder name (`default`, `video-studio`) names its package.
fn package_name(template: &TemplateRef) -> TemplateRef {
    builtins()
        .iter()
        .find(|builtin| builtin.answers(template))
        .map_or_else(
            || template.clone(),
            |builtin| TemplateRef {
                name: builtin.template.package.document.name.clone(),
                version: template.version.clone(),
            },
        )
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
    let (package, provenance) = crate::service::team_exchange::resolve(&package_name(requested))
        .ok_or_else(|| TemplateStartRefusal::NotFound {
            template: requested.clone(),
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
