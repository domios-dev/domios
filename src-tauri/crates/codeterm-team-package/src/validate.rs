use std::collections::{BTreeMap, BTreeSet};

use crate::diagnostics::{DiagnosticCode as Code, Sink};
use crate::document::*;
use crate::paths;
use crate::yaml::Locations;
use crate::{CheckOptions, Limits};

struct Check<'a> {
    sink: &'a mut Sink,
    locations: &'a Locations,
    limits: &'a Limits,
}

impl Check<'_> {
    fn error(&mut self, code: Code, path: &str, message: String) {
        let mark = self.locations.at(path);
        self.sink.error(code, path, mark, message);
    }

    fn path(&mut self, path: &str, at: &str) -> bool {
        match paths::check(path, self.limits.max_path_bytes) {
            Ok(()) => true,
            Err((code, message)) => {
                self.error(code, at, message);
                false
            }
        }
    }
}

fn charset(text: &str, allowed: fn(u8) -> bool) -> bool {
    !text.is_empty() && text.bytes().all(allowed)
}

fn lower_digit_dash(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
}

/// `<owner>/<slug>`: owner `[a-z0-9.-]+` in reverse-DNS style, slug `[a-z0-9-]+`.
pub fn is_package_name(name: &str) -> bool {
    let Some((owner, slug)) = name.split_once('/') else {
        return false;
    };
    name.len() <= 128
        && charset(owner, |byte| lower_digit_dash(byte) || byte == b'.')
        && charset(slug, lower_digit_dash)
}

/// A harness id, `[a-z0-9-]+`.
pub fn is_harness(harness: &str) -> bool {
    harness.len() <= 64 && charset(harness, lower_digit_dash)
}

/// A role name, `[a-z][a-z0-9_]*`.
pub fn is_role_name(name: &str) -> bool {
    name.len() <= 64
        && name.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && charset(name, |byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
        })
}

fn numeric(part: &str) -> bool {
    !part.is_empty()
        && part.bytes().all(|byte| byte.is_ascii_digit())
        && (part == "0" || !part.starts_with('0'))
}

fn identifiers(text: &str) -> bool {
    text.split('.').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    })
}

/// Semantic Versioning 2.0.0.
pub fn is_semver(version: &str) -> bool {
    let (rest, build) = match version.split_once('+') {
        Some((rest, build)) => (rest, Some(build)),
        None => (version, None),
    };
    let (core, pre) = match rest.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (rest, None),
    };
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| numeric(part))
        && pre.is_none_or(|pre| {
            identifiers(pre)
                && pre
                    .split('.')
                    .all(|part| !part.bytes().all(|byte| byte.is_ascii_digit()) || numeric(part))
        })
        && build.is_none_or(identifiers)
}

fn is_token(text: &str, max: usize) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= max
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-'
        })
}

fn is_url(text: &str) -> bool {
    (text.starts_with("https://") || text.starts_with("http://"))
        && text.len() > "https://".len()
        && !text.chars().any(char::is_whitespace)
}

/// `<positive integer>(m|h)`, in seconds.
fn duration_seconds(text: &str) -> Option<u64> {
    let unit = text.chars().last()?;
    let multiplier = match unit {
        'm' => 60,
        'h' => 3_600,
        _ => return None,
    };
    let value: u64 = text[..text.len() - 1].parse().ok()?;
    (value > 0
        && text[..text.len() - 1]
            .bytes()
            .all(|byte| byte.is_ascii_digit()))
    .then_some(value * multiplier)
}

/// A `<min>..<max>` schedule window, as `(min, max)` seconds.
pub fn schedule_window(text: &str) -> Option<(u64, u64)> {
    let (min, max) = text.split_once("..")?;
    let window = (duration_seconds(min)?, duration_seconds(max)?);
    (window.0 <= window.1).then_some(window)
}

/// `<harness>:<model>`, for example `claude:opus`; the model token is provider-defined.
pub fn is_model_id(model: &str) -> bool {
    let Some((harness, id)) = model.split_once(':') else {
        return false;
    };
    is_harness(harness)
        && !id.is_empty()
        && !id
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
}

fn role_path(name: &str) -> String {
    format!("roles.{name}")
}

fn identity(check: &mut Check<'_>, document: &TeamDocument) {
    if !is_package_name(&document.name) {
        check.error(
            Code::InvalidName,
            "name",
            format!(
                "`{}` is not `<reverse-dns>/<slug>`, for example `dev.example/research`",
                document.name
            ),
        );
    }
    if !is_semver(&document.version) {
        check.error(
            Code::InvalidVersion,
            "version",
            format!("`{}` is not a semantic version", document.version),
        );
    }
    if document.description.trim().is_empty() {
        check.error(
            Code::InvalidValue,
            "description",
            "description cannot be empty".into(),
        );
    }
    if let Some(lineage) = &document.derived_from {
        if !is_package_name(&lineage.name) {
            check.error(
                Code::InvalidDerivedFrom,
                "derived_from.name",
                format!("`{}` is not a reverse-DNS package name", lineage.name),
            );
        }
        if !is_semver(&lineage.version) {
            check.error(
                Code::InvalidDerivedFrom,
                "derived_from.version",
                format!("`{}` is not a semantic version", lineage.version),
            );
        }
    }
    for (index, author) in document.authors.iter().enumerate() {
        if author
            .email
            .as_deref()
            .is_some_and(|email| !email.contains('@') || email.chars().any(char::is_whitespace))
        {
            check.error(
                Code::InvalidEmail,
                &format!("authors[{index}].email"),
                "not an email address".into(),
            );
        }
        if author.url.as_deref().is_some_and(|url| !is_url(url)) {
            check.error(
                Code::InvalidUrl,
                &format!("authors[{index}].url"),
                "not an http(s) URL".into(),
            );
        }
    }
    if let Some(links) = &document.links {
        for (field, url) in [
            ("homepage", &links.homepage),
            ("repository", &links.repository),
            ("issues", &links.issues),
            ("docs", &links.docs),
        ] {
            if url.as_deref().is_some_and(|url| !is_url(url)) {
                check.error(
                    Code::InvalidUrl,
                    &format!("links.{field}"),
                    "not an http(s) URL".into(),
                );
            }
        }
    }
    if let Some(icon) = &document.icon {
        check.path(icon, "icon");
    }
}

fn asks(check: &mut Check<'_>, document: &TeamDocument) {
    for (index, harness) in document
        .requires
        .iter()
        .flat_map(|requires| requires.harnesses.iter())
        .enumerate()
    {
        if !is_harness(harness) {
            check.error(
                Code::InvalidHarness,
                &format!("requires.harnesses[{index}]"),
                format!("`{harness}` is not a harness id"),
            );
        }
    }
    for (key, input) in document.inputs.iter() {
        let path = format!("inputs.{key}");
        if !is_token(key, 64) || key.contains('-') {
            check.error(
                Code::InvalidInputName,
                &path,
                format!("`{key}` must be lower_snake_case"),
            );
        }
        if let Some(default) = input.default_value() {
            if !input.choices().is_empty()
                && !input.choices().iter().any(|choice| choice == default)
            {
                check.error(
                    Code::InvalidValue,
                    &format!("{path}.default"),
                    format!("default `{default}` is not one of the choices"),
                );
            }
        }
    }
    if let Some(PackageNetwork::Hosts(hosts)) = document
        .permissions
        .as_ref()
        .and_then(|permissions| permissions.network.as_ref())
    {
        for (index, host) in hosts.iter().enumerate() {
            if host.is_empty()
                || host.contains("://")
                || host.chars().any(|c| c.is_whitespace() || c == '/')
            {
                check.error(
                    Code::InvalidValue,
                    &format!("permissions.network[{index}]"),
                    format!("`{host}` is not a host name or wildcard"),
                );
            }
        }
    }
    let mut secrets = BTreeSet::new();
    for (index, secret) in document.secrets.iter().enumerate() {
        let path = format!("secrets[{index}].name");
        let valid = secret
            .name
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_uppercase)
            && secret
                .name
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_');
        if !valid {
            check.error(
                Code::InvalidSecretName,
                &path,
                format!("`{}` must be UPPER_SNAKE_CASE", secret.name),
            );
        }
        if !secrets.insert(secret.name.as_str()) {
            check.error(
                Code::DuplicateSecret,
                &path,
                format!("secret `{}` is declared twice", secret.name),
            );
        }
    }
}

fn payload(check: &mut Check<'_>, document: &TeamDocument) {
    let mut declared = BTreeSet::new();
    for (index, file) in document.files.iter().enumerate() {
        let path = format!("files[{index}].path");
        if check.path(&file.path, &path) && !declared.insert(file.path.as_str()) {
            check.error(
                Code::DuplicatePath,
                &path,
                format!("`{}` is declared twice", file.path),
            );
        }
    }
    for (index, skill) in document.skills.iter().enumerate() {
        match skill {
            PackageSkill::Path { path } => {
                check.path(path, &format!("skills[{index}].path"));
            }
            PackageSkill::Inline(inline) => {
                if !is_token(&inline.name, 64) {
                    check.error(
                        Code::InvalidValue,
                        &format!("skills[{index}].name"),
                        format!("`{}` must be a lowercase skill name", inline.name),
                    );
                }
                for (file_index, file) in inline.files.iter().enumerate() {
                    check.path(
                        &file.path,
                        &format!("skills[{index}].files[{file_index}].path"),
                    );
                }
            }
        }
    }
    for (index, automation) in document.automation.iter().enumerate() {
        if let PackageAutomation::Path { path, .. } = automation {
            check.path(path, &format!("automation[{index}].path"));
        }
    }
}

fn role(check: &mut Check<'_>, document: &TeamDocument, name: &str, role: &TeamRole) {
    let path = role_path(name);
    if !is_role_name(name) {
        check.error(
            Code::InvalidRoleName,
            &path,
            format!("`{name}` must match [a-z][a-z0-9_]*"),
        );
    } else if role.kind == RoleKind::Worker && is_builtin_manager(name) {
        check.error(
            Code::ReservedWorkerName,
            &format!("{path}.kind"),
            format!("`{name}` is a built-in manager name, so its kind must be `manager`"),
        );
    }
    if role.instructions.is_none() && role.instructions_file.is_none() {
        let mark = check.locations.at(&path);
        check.sink.error(
            Code::MissingInstructions,
            &format!("{path}.instructions"),
            mark,
            format!("`{name}` needs `instructions` or `instructions_file`"),
        );
    }
    match &role.start {
        Some(RoleStart::Schedule { schedule }) if schedule_window(schedule).is_none() => check
            .error(
            Code::InvalidSchedule,
            &format!("{path}.start.schedule"),
            format!(
                "`{schedule}` is not `<min>..<max>` with positive m or h durations and min <= max"
            ),
        ),
        _ => {}
    }
    let declared = |mode: StartMode| {
        role.start
            .as_ref()
            .is_some_and(|start| *start != RoleStart::Mode(mode))
    };
    if name == ORCHESTRATOR && declared(StartMode::WithTeam) {
        check.error(
            Code::InvalidStart,
            &format!("{path}.start"),
            "the orchestrator always starts with the Team".into(),
        );
    } else if name != ORCHESTRATOR && role.kind == RoleKind::Worker && declared(StartMode::OnDemand)
    {
        check.error(
            Code::InvalidStart,
            &format!("{path}.start"),
            "a worker takes one task and ends, so it starts `on_demand`".into(),
        );
    }
    match &role.model {
        Some(ModelPreference::List(models)) if models.is_empty() => check.error(
            Code::InvalidModel,
            &format!("{path}.model"),
            "a model list needs at least one `harness:model` id; omit `model` instead".into(),
        ),
        Some(ModelPreference::List(models)) => {
            for (index, model) in models.iter().enumerate() {
                if !is_model_id(model) {
                    check.error(
                        Code::InvalidModel,
                        &format!("{path}.model[{index}]"),
                        format!("`{model}` is not `harness:model`"),
                    );
                }
            }
        }
        Some(ModelPreference::SameAs { same_as }) if document.role(same_as).is_none() => check
            .error(
                Code::UnknownSameAs,
                &format!("{path}.model.same_as"),
                format!("`{same_as}` is not a role in this package"),
            ),
        _ => {}
    }
    if role
        .reasoning
        .as_deref()
        .is_some_and(|reasoning| !is_token(reasoning, 32))
    {
        check.error(
            Code::InvalidValue,
            &format!("{path}.reasoning"),
            "reasoning is a lowercase effort level".into(),
        );
    }
    for (index, tool) in role.tools.iter().enumerate() {
        let valid = tool
            .strip_prefix("mcp:")
            .is_some_and(|name| !name.is_empty() && !name.chars().any(char::is_whitespace));
        if !valid {
            check.error(
                Code::InvalidTool,
                &format!("{path}.tools[{index}]"),
                format!("`{tool}` is not `mcp:<registry name>`"),
            );
        }
    }
    for (index, file) in role.files.iter().enumerate() {
        if !document.files.iter().any(|declared| &declared.path == file) {
            check.error(
                Code::UnknownFileReference,
                &format!("{path}.files[{index}]"),
                format!("`{file}` is not declared in top-level `files`"),
            );
        }
    }
    if let Some(file) = &role.instructions_file {
        check.path(file, &format!("{path}.instructions_file"));
    }
}

fn same_as_cycles(check: &mut Check<'_>, document: &TeamDocument) {
    let targets: BTreeMap<String, String> = document
        .roles()
        .filter_map(|(name, role)| match &role.model {
            Some(ModelPreference::SameAs { same_as }) => {
                Some((name.to_string(), same_as.clone()))
            }
            _ => None,
        })
        .collect();
    for start in targets.keys() {
        let mut current = start.as_str();
        let mut seen = BTreeSet::new();
        while let Some(next) = targets.get(current) {
            if next == start {
                check.error(
                    Code::SameAsCycle,
                    &format!("{}.model.same_as", role_path(start)),
                    format!("`same_as` from `{start}` leads back to itself"),
                );
                break;
            }
            if !seen.insert(next.as_str()) {
                break;
            }
            current = next.as_str();
        }
    }
}

/// Semantic rules over a structurally valid document.
pub(crate) fn document(
    document: &TeamDocument,
    locations: &Locations,
    options: &CheckOptions,
    sink: &mut Sink,
) {
    let mut check = Check {
        sink,
        locations,
        limits: &options.limits,
    };
    identity(&mut check, document);
    asks(&mut check, document);
    payload(&mut check, document);
    for (name, declared) in document.roles() {
        role(&mut check, document, name, declared);
    }
    same_as_cycles(&mut check, document);
}
