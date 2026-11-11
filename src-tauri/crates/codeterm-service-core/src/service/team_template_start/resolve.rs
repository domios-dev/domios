use std::sync::OnceLock;

use codeterm_team_package::contracts::{TemplateProvenance, TemplateRef, TemplateStartRefusal};
use codeterm_team_package::{builtin, CheckOptions, Checked, Package, PackageDiagnostic};
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
            Some(package) => Ok(Self {
                package,
                provenance,
                warnings: report.warnings().cloned().collect(),
            }),
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
                let checked = builtin::check(&folder, &CheckOptions::domios())?;
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

/// An absent template is the builtin default; a named one is a builtin by package or folder name.
pub fn resolve(template: Option<&TemplateRef>) -> Result<ResolvedTemplate, TemplateStartRefusal> {
    let default_ref = TemplateRef {
        name: DEFAULT_PACKAGE.to_string(),
        version: None,
    };
    let template = template.unwrap_or(&default_ref);
    builtins()
        .iter()
        .find(|builtin| builtin.answers(template))
        .map(|builtin| builtin.template.clone())
        .ok_or_else(|| TemplateStartRefusal::NotFound {
            template: template.clone(),
        })
}
