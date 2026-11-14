//! The one exporter and importer. Distill, export and Save as template all run one
//! pipeline: scrub, check through the reference validator, project onto the requested
//! form and write. Import checks every form before anything reaches My templates.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use codeterm_team_package::contracts::{
    declared_asks, ScrubReport, TemplateDeleteRequest, TemplateDeleteResult,
    TemplateDistillRequest, TemplateDistillResult, TemplateExportRequest, TemplateExportResult,
    TemplateImportResult, TemplateProvenance, TemplateRef, TemplateShareCard, TemplateShareState,
    TemplateStartRefusal, TemplateSummary,
};
use codeterm_team_package::{
    check_entries, check_path, check_yaml, payload_items, CheckOptions, Package,
    PackageCheckReport, PackageDiagnostic, PackageFormat,
};
use serde::Serialize;
use serde_json::Value;

use super::ServiceContext;

pub mod distill;
pub mod forms;
pub mod library;
pub mod scrub;

use library::{Saved, TemplateLibrary};
use scrub::ScrubContext;

/// A refused exchange: a bad request, or a package the validator rejected.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExchangeError {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<PackageDiagnostic>,
}

pub const INVALID_REQUEST: &str = "invalid_request";
pub const INVALID_PACKAGE: &str = "invalid_package";
pub const NOT_FOUND: &str = "not_found";

impl ExchangeError {
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: INVALID_REQUEST,
            message: message.into(),
            diagnostics: Vec::new(),
        }
    }

    fn not_found(template: &TemplateRef) -> Self {
        Self {
            code: NOT_FOUND,
            message: format!(
                "template `{}` is not builtin or in My templates",
                label(template)
            ),
            diagnostics: Vec::new(),
        }
    }

    fn package(report: PackageCheckReport) -> Self {
        let first = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.is_error())
            .map(|diagnostic| diagnostic.message.clone())
            .unwrap_or_else(|| "the package is not valid".to_string());
        Self {
            code: INVALID_PACKAGE,
            message: first,
            diagnostics: report.diagnostics,
        }
    }
}

impl std::fmt::Display for ExchangeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

type Outcome = Result<Value, ExchangeError>;

fn label(template: &TemplateRef) -> String {
    match &template.version {
        Some(version) => format!("{}@{version}", template.name),
        None => template.name.clone(),
    }
}

fn parse<T: serde::de::DeserializeOwned>(params: Option<&Value>) -> Result<T, ExchangeError> {
    serde_json::from_value(params.cloned().unwrap_or(Value::Null))
        .map_err(|error| ExchangeError::invalid(error.to_string()))
}

fn to_value<T: Serialize>(value: &T) -> Outcome {
    serde_json::to_value(value).map_err(|error| ExchangeError::invalid(error.to_string()))
}

/// Every builtin package, checked once.
pub fn builtin_packages() -> &'static [Package] {
    static PACKAGES: OnceLock<Vec<Package>> = OnceLock::new();
    PACKAGES.get_or_init(|| {
        super::team_catalog::bundled()
            .iter()
            .map(|(_, package)| package.clone())
            .collect()
    })
}

/// The builtin default package every Team falls back to.
pub fn default_package() -> Option<&'static Package> {
    builtin_packages()
        .iter()
        .find(|package| package.document.name == DEFAULT_TEMPLATE)
}

pub const DEFAULT_TEMPLATE: &str = "dev.domios/default";

/// A builtin or saved template by name and optional version.
pub fn resolve(template: &TemplateRef) -> Option<(Package, TemplateProvenance)> {
    resolve_in(TemplateLibrary::global(), template)
}

/// [`resolve`] against one library.
pub fn resolve_in(
    library: &TemplateLibrary,
    template: &TemplateRef,
) -> Option<(Package, TemplateProvenance)> {
    super::team_catalog::resolve_in(
        super::team_channels::TeamChannelStore::global(),
        library,
        template,
    )
    .map(|entry| (entry.package, entry.provenance))
}

/// The exporting machine's private bindings: workspace folders, home and machine names.
fn scrub_context(svc: &ServiceContext, machines: Vec<String>, live_team: bool) -> ScrubContext {
    let roots = svc
        .list_workspaces()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|workspace| workspace.cwd)
        .collect();
    ScrubContext::local(roots, machines, live_team)
}

/// What one pipeline run produced.
pub struct Exported {
    pub package: Package,
    pub yaml: String,
    pub check: PackageCheckReport,
    pub scrub: ScrubReport,
    pub path: Option<String>,
}

/// Scrub, check, project and write. Nothing is written without a destination or when a
/// check fails.
pub fn run_pipeline(
    mut package: Package,
    context: &ScrubContext,
    format: PackageFormat,
    destination: Option<PathBuf>,
) -> Result<Exported, ExchangeError> {
    let scrub = scrub::scrub_package(&mut package, context);
    let package = forms::canonical_for(package, format).map_err(ExchangeError::invalid)?;
    let canonical = check_entries(
        PackageFormat::Folder,
        package.archive_entries(),
        Vec::new(),
        &CheckOptions::default(),
    );
    let Some(package) = canonical.package else {
        return Err(ExchangeError::package(canonical.report));
    };
    let (form, written) = forms::render(&package, format).map_err(ExchangeError::invalid)?;
    let yaml = form.to_yaml();
    let check = match format {
        PackageFormat::Yaml => check_yaml(&yaml, &CheckOptions::default()).report,
        _ => {
            let mut report = canonical.report;
            report.format = format;
            report
        }
    };
    if !check.valid {
        return Err(ExchangeError::package(check));
    }
    let path = match destination {
        Some(target) => {
            forms::write(&target, &written).map_err(ExchangeError::invalid)?;
            Some(target.display().to_string())
        }
        None => None,
    };
    Ok(Exported {
        package,
        yaml,
        check,
        scrub,
        path,
    })
}

/// The exports folder a destination-less export writes into.
pub fn exports_dir() -> PathBuf {
    crate::paths::data_dir().join("team-exports")
}

fn destination(
    out: Option<&str>,
    package: &Package,
    format: PackageFormat,
) -> Result<PathBuf, ExchangeError> {
    match out {
        Some(out) => {
            let path = Path::new(out);
            if !path.is_absolute() {
                return Err(ExchangeError::invalid("`out` must be an absolute path"));
            }
            Ok(path.to_path_buf())
        }
        None => Ok(exports_dir().join(forms::default_name(package, format))),
    }
}

fn reindex(svc: &ServiceContext, name: &str) {
    if let Err(error) = super::team_template_recall::index_template(svc, name) {
        log::warn!("[team-templates] recall index for {name} not updated: {error}");
    }
}

/// `team export`: a builtin or saved template through the one pipeline.
pub fn export(svc: &ServiceContext, params: Option<&Value>) -> Outcome {
    let request: TemplateExportRequest = parse(params)?;
    let Some(template) = request.template else {
        return Err(ExchangeError::invalid(
            "name a template; a live Team is baked with `team distill`",
        ));
    };
    let (package, _) = resolve(&template).ok_or_else(|| ExchangeError::not_found(&template))?;
    let target = destination(request.out.as_deref(), &package, request.format)?;
    let exported = run_pipeline(
        package,
        &scrub_context(svc, Vec::new(), false),
        request.format,
        Some(target),
    )?;
    to_value(&TemplateExportResult {
        format: request.format,
        check: exported.check,
        scrub: exported.scrub,
        path: exported.path,
        yaml: Some(exported.yaml),
    })
}

/// `team distill`: bakes a live Team and exports, saves or only reviews the result.
pub fn distill(svc: &ServiceContext, params: Option<&Value>) -> Outcome {
    let request: TemplateDistillRequest = parse(params)?;
    if request.name.split('/').next() == Some(library::BUILTIN_OWNER) {
        return Err(ExchangeError::invalid(format!(
            "`{}/` names builtin templates; bake under your own name",
            library::BUILTIN_OWNER
        )));
    }
    let registrations = super::team_roles::registrations(Some(request.team_id.as_str()));
    let pinned = distill::source_identity(&registrations);
    let base = pinned
        .as_ref()
        .and_then(|pinned| {
            resolve(&TemplateRef {
                name: pinned.name.clone(),
                version: Some(pinned.version.clone()),
            })
        })
        .map(|(package, _)| package)
        .or_else(|| default_package().cloned())
        .ok_or_else(|| ExchangeError::invalid("the builtin default template is missing"))?;
    let descriptions: BTreeMap<String, String> = base
        .document
        .files
        .iter()
        .filter_map(|file| Some((file.path.clone(), file.description.clone()?)))
        .collect();
    let collected = super::team_template_payload::collect(svc, &request.team_id, &descriptions)
        .map_err(ExchangeError::invalid)?;
    let history = collected.history_excluded;
    let machines: Vec<String> = registrations
        .iter()
        .filter_map(|registration| registration.placement_peer.clone())
        .collect();
    let placements = machines.len() as u32;
    let baked = distill::bake(&request, &registrations, &base, collected);
    let baked = super::team_template_payload::retain_opted_in(&baked, &request.opt_out);
    let target = if request.dry_run {
        None
    } else {
        Some(destination(request.out.as_deref(), &baked, request.format)?)
    };
    let mut exported = run_pipeline(
        baked,
        &scrub_context(svc, machines, true),
        request.format,
        target,
    )?;
    exported.scrub.machines_removed += placements;
    exported.scrub.history_removed += history;
    let saved = if request.save && !request.dry_run {
        Some(save(svc, &exported.package, TemplateProvenance::Saved)?.0)
    } else {
        None
    };
    to_value(&TemplateDistillResult {
        includes: payload_items(&exported.package),
        yaml: exported.yaml,
        check: exported.check,
        scrub: exported.scrub,
        path: exported.path,
        saved,
    })
}

fn save(
    svc: &ServiceContext,
    package: &Package,
    provenance: TemplateProvenance,
) -> Result<
    (
        TemplateSummary,
        Option<codeterm_team_package::contracts::TemplateUpdateDiff>,
    ),
    ExchangeError,
> {
    let update = match TemplateLibrary::global()
        .save(package, provenance)
        .map_err(ExchangeError::invalid)?
    {
        Saved::Created { update } => update,
        Saved::Unchanged => None,
    };
    reindex(svc, &package.document.name);
    Ok((TemplateLibrary::global().summary(package), update))
}

/// Refuses a received package that still carries a secret value; the sender exports again.
fn refuse_secret_values(package: &Package) -> Result<(), ExchangeError> {
    let mut probe = package.clone();
    let report = scrub::scrub_package(&mut probe, &ScrubContext::default());
    if report.secret_values_removed > 0 {
        return Err(ExchangeError::invalid(
            "the package carries a secret value; ask the sender to export it again so secrets travel as names only",
        ));
    }
    Ok(())
}

/// Checks a package file or folder in any form; nothing is saved unless it is valid and safe.
/// Links inside a package are refused by the reader; the path the owner named may be one.
pub fn check_import(path: &Path) -> Result<Package, ExchangeError> {
    check_import_report(path).map(|(package, _)| package)
}

fn check_import_report(path: &Path) -> Result<(Package, PackageCheckReport), ExchangeError> {
    if !path.is_absolute() {
        return Err(ExchangeError::invalid("`path` must be an absolute path"));
    }
    let checked = check_path(path, &CheckOptions::default());
    let package = checked
        .package
        .ok_or_else(|| ExchangeError::package(checked.report.clone()))?;
    super::team_template_start::refuse_host_owned_roles(&package).map_err(|refusal| {
        let role = match refusal {
            TemplateStartRefusal::HostOwnedRole { role } => role,
            other => format!("{other:?}"),
        };
        ExchangeError::invalid(format!(
            "role `{role}` is owned by Domios and cannot be a package role"
        ))
    })?;
    refuse_secret_values(&package)?;
    Ok((package, checked.report))
}

#[derive(serde::Deserialize)]
struct ImportParams {
    path: String,
    /// The friendship a share card received the package through.
    #[serde(default)]
    from_friend: Option<String>,
}

/// `team import`: a file, archive or folder into My templates, unverified.
pub fn import(svc: &ServiceContext, params: Option<&Value>) -> Outcome {
    let request: ImportParams = parse(params)?;
    let received_from = match request.from_friend.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(friendship) => Some(svc.neighbor_friend_display_name(friendship).ok_or_else(
            || ExchangeError::invalid(format!("`{friendship}` names no current friend")),
        )?),
    };
    let path = PathBuf::from(&request.path);
    let (package, check) = check_import_report(&path)?;
    let (mut template, update) = save(svc, &package, TemplateProvenance::Imported)?;
    if let Some(friend) = received_from {
        TemplateLibrary::global()
            .mark_received(&template.name, &template.version, &friend)
            .map_err(ExchangeError::invalid)?;
        template.received_from = Some(friend);
    }
    to_value(&TemplateImportResult {
        template,
        check,
        update,
    })
}

/// Removes a template from My templates and from recall; builtins cannot be removed.
pub fn delete(svc: &ServiceContext, params: Option<&Value>) -> Outcome {
    let request: TemplateDeleteRequest = parse(params)?;
    if request.template.name.split('/').next() == Some(library::BUILTIN_OWNER) {
        return Err(ExchangeError::invalid(
            "builtin templates cannot be deleted",
        ));
    }
    let removed = TemplateLibrary::global()
        .delete(&request.template)
        .map_err(ExchangeError::invalid)?;
    reindex(svc, &request.template.name);
    to_value(&TemplateDeleteResult { removed })
}

#[derive(serde::Deserialize)]
struct CardParams {
    file_id: String,
    path: String,
}

/// The Neighborhood card for a received package file: checked, never saved or started.
pub fn share_card(params: Option<&Value>) -> Outcome {
    let request: CardParams = parse(params)?;
    let package = check_import(Path::new(&request.path))?;
    let saved = TemplateLibrary::global()
        .load(&package.document.name, Some(&package.document.version))
        .is_some();
    to_value(&TemplateShareCard {
        file_id: request.file_id,
        template: TemplateSummary::from_package(&package, TemplateProvenance::Imported),
        roles: package.document.roles.len() as u32,
        asks: declared_asks(&package),
        state: if saved {
            TemplateShareState::Previewed
        } else {
            TemplateShareState::Unverified
        },
    })
}

#[cfg(test)]
#[path = "team_exchange/tests.rs"]
mod tests;
