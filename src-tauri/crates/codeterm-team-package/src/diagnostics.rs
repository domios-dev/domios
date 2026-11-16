use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::PackageFormat;

/// Stable diagnostic codes; their snake_case spelling is the protocol contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCode {
    Unreadable,
    YamlSyntax,
    TabIndentation,
    ControlCharacter,
    MultipleDocuments,
    EmptyDocument,
    YamlAnchor,
    YamlAlias,
    YamlTag,
    DuplicateKey,
    ComplexKey,
    TooLarge,
    TooDeep,
    TooManyItems,
    StringTooLong,
    ExpectedMapping,
    ExpectedList,
    ExpectedString,
    ExpectedBoolean,
    MissingField,
    UnknownField,
    RemovedField,
    RemovedInherit,
    PlanTemplateUnsupported,
    ConflictingFields,
    InvalidValue,
    InvalidName,
    InvalidVersion,
    InvalidDerivedFrom,
    InvalidRoleName,
    MissingRoleKind,
    ReservedWorkerName,
    MissingInstructions,
    InvalidInputName,
    InvalidSecretName,
    DuplicateSecret,
    InvalidHarness,
    InvalidModel,
    InvalidTool,
    InvalidSchedule,
    InvalidStart,
    InvalidUrl,
    InvalidEmail,
    UnknownSameAs,
    SameAsCycle,
    InvalidPath,
    AbsolutePath,
    PathEscape,
    DuplicatePath,
    MissingFile,
    UnknownFileReference,
    FileConflict,
    IconNotImage,
    SvgScript,
    SvgExternalReference,
    BinaryInline,
    DuplicateSkill,
    UnresolvedSkill,
    MissingRoot,
    DuplicateEntry,
    UnreferencedEntry,
    LinkedEntry,
    ArchiveError,
    UnsupportedEntry,
    TooManyEntries,
    EntryTooLarge,
    NotUtf8,
}

impl DiagnosticCode {
    pub fn name(self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

/// One finding about a portable package. `file` is package-relative; `path` names the YAML node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PackageDiagnostic {
    pub code: DiagnosticCode,
    pub severity: DiagnosticSeverity,
    pub file: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    pub message: String,
}

impl PackageDiagnostic {
    pub fn is_error(&self) -> bool {
        self.severity == DiagnosticSeverity::Error
    }

    /// `file:line:column: severity[code] path: message`, the line `team check` prints.
    pub fn render(&self, file_label: &str) -> String {
        let position = match (self.line, self.column) {
            (Some(line), Some(column)) => format!(":{line}:{column}"),
            (Some(line), None) => format!(":{line}"),
            _ => String::new(),
        };
        let severity = match self.severity {
            DiagnosticSeverity::Error => "error",
            DiagnosticSeverity::Warning => "warning",
        };
        let path = if self.path.is_empty() {
            String::new()
        } else {
            format!(" {}", self.path)
        };
        format!(
            "{file_label}{position}: {severity}[{}]{path}: {}",
            self.code.name(),
            self.message
        )
    }
}

/// The validator's verdict on one package in one form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PackageCheckReport {
    pub format: PackageFormat,
    pub valid: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub diagnostics: Vec<PackageDiagnostic>,
}

impl PackageCheckReport {
    pub fn errors(&self) -> impl Iterator<Item = &PackageDiagnostic> {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.is_error())
    }

    pub fn warnings(&self) -> impl Iterator<Item = &PackageDiagnostic> {
        self.diagnostics
            .iter()
            .filter(|diagnostic| !diagnostic.is_error())
    }

    /// The pretty-printed JSON document the standalone CLI and wasm validators emit.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// One rendered line per diagnostic, then the verdict; archive and folder files are
    /// labelled `package/file`.
    pub fn render(&self, package: &str) -> String {
        let mut out = String::new();
        for diagnostic in &self.diagnostics {
            let label = match self.format {
                PackageFormat::Yaml => package.to_string(),
                _ if diagnostic.file.is_empty() => package.to_string(),
                _ => format!("{package}/{}", diagnostic.file),
            };
            out.push_str(&diagnostic.render(&label));
            out.push('\n');
        }
        let identity = match (&self.name, &self.version) {
            (Some(name), Some(version)) => format!(" {name}@{version}"),
            _ => String::new(),
        };
        let errors = self.errors().count();
        let warnings = self.warnings().count();
        if self.valid {
            out.push_str(&format!("valid{identity} ({warnings} warning(s))\n"));
        } else {
            out.push_str(&format!(
                "invalid{identity}: {errors} error(s), {warnings} warning(s)\n"
            ));
        }
        out
    }
}

/// Collects diagnostics for one file while a package is read.
pub(crate) struct Sink {
    pub(crate) file: String,
    pub(crate) diagnostics: Vec<PackageDiagnostic>,
}

impl Sink {
    pub(crate) fn new(file: impl Into<String>) -> Self {
        Self {
            file: file.into(),
            diagnostics: Vec::new(),
        }
    }

    pub(crate) fn push(
        &mut self,
        code: DiagnosticCode,
        severity: DiagnosticSeverity,
        path: &str,
        mark: Option<crate::yaml::Mark>,
        message: impl Into<String>,
    ) {
        self.diagnostics.push(PackageDiagnostic {
            code,
            severity,
            file: self.file.clone(),
            path: path.to_string(),
            line: mark.map(|mark| mark.line),
            column: mark.map(|mark| mark.column),
            message: message.into(),
        });
    }

    pub(crate) fn error(
        &mut self,
        code: DiagnosticCode,
        path: &str,
        mark: Option<crate::yaml::Mark>,
        message: impl Into<String>,
    ) {
        self.push(code, DiagnosticSeverity::Error, path, mark, message);
    }

    pub(crate) fn warning(
        &mut self,
        code: DiagnosticCode,
        path: &str,
        mark: Option<crate::yaml::Mark>,
        message: impl Into<String>,
    ) {
        self.push(code, DiagnosticSeverity::Warning, path, mark, message);
    }
}
