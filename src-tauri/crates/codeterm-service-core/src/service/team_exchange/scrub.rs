//! The privacy projection every export applies. It rewrites only private bindings
//! (personal paths, machine names, secret values) and keeps the surrounding prose, unlike
//! the desk sanitizer's same-size filler. Secret detection is `codeterm_util::redact`.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use codeterm_team_package::contracts::ScrubReport;
use codeterm_team_package::{
    Package, PackageAutomation, PackageInput, PackageSecret, PackageSkill,
};
use regex::Regex;

pub const WORKSPACE: &str = "{workspace}";
pub const MACHINE: &str = "{machine}";
/// Opens `[secret NAME]`, the text a removed value leaves; it is never interpolated.
const SECRET_PLACEHOLDER: &str = "[secret";

/// The private bindings of the exporting machine.
#[derive(Debug, Clone, Default)]
pub struct ScrubContext {
    /// Workspace folders and the home folder; any path under them becomes `{workspace}`.
    pub personal_roots: Vec<String>,
    /// This machine's and its peers' names.
    pub machines: Vec<String>,
}

impl ScrubContext {
    /// The exporting machine: its workspaces and home folder, plus its host name when the
    /// texts came from a live Team (a template's own prose never names this machine).
    pub fn local(workspace_roots: Vec<String>, machines: Vec<String>, live_team: bool) -> Self {
        let mut personal_roots = workspace_roots;
        if let Some(home) = dirs::home_dir() {
            personal_roots.push(home.display().to_string());
        }
        let mut machines = machines;
        if live_team {
            if let Ok(name) = hostname::get() {
                machines.push(name.to_string_lossy().into_owned());
            }
        }
        Self {
            personal_roots,
            machines,
        }
    }
}

fn assignment() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r#"\b([A-Z][A-Z0-9_]{2,})(\s*[:=]\s*)(["']?)([^\s"'\[{<$][^\s"']{3,})"#)
            .expect("assignment pattern")
    })
}

fn long_token() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"\S{20,}").expect("token pattern"))
}

/// True when `redact` treats a variable of this name as a secret.
fn secret_name(name: &str) -> bool {
    let probe = format!("{name}=withheld");
    codeterm_util::redact::text(&probe) != probe
}

fn path_variants(root: &str) -> Vec<String> {
    let trimmed = root.trim_end_matches(['/', '\\']);
    let forward = trimmed.replace('\\', "/");
    let backward = trimmed.replace('/', "\\");
    let mut variants = vec![trimmed.to_string(), forward, backward];
    variants.sort();
    variants.dedup();
    variants
}

pub struct Scrubber {
    roots: Vec<String>,
    machines: Vec<Regex>,
    secrets: BTreeSet<String>,
    pub report: ScrubReport,
    asks: Vec<PackageSecret>,
}

impl Scrubber {
    pub fn new(context: &ScrubContext) -> Self {
        let mut roots: Vec<String> = context
            .personal_roots
            .iter()
            .flat_map(|root| path_variants(root))
            .filter(|root| root.len() >= 4 && root.contains(['/', '\\']))
            .collect();
        roots.sort_by(|left, right| right.len().cmp(&left.len()).then(left.cmp(right)));
        roots.dedup();
        let mut names: Vec<&str> = context
            .machines
            .iter()
            .map(|name| name.trim())
            .filter(|name| name.len() >= 3)
            .collect();
        names.sort_by(|left, right| right.len().cmp(&left.len()).then(left.cmp(right)));
        names.dedup();
        let machines = names
            .into_iter()
            .filter_map(|name| Regex::new(&format!(r"(?i)\b{}\b", regex::escape(name))).ok())
            .collect();
        Self {
            roots,
            machines,
            secrets: BTreeSet::new(),
            report: ScrubReport::default(),
            asks: Vec::new(),
        }
    }

    /// Rewrites the private bindings in one text; `at` names where it was found.
    pub fn text(&mut self, input: &str, at: &str) -> String {
        let mut value = input.to_string();
        for root in &self.roots {
            let count = value.matches(root.as_str()).count();
            if count > 0 {
                value = value.replace(root.as_str(), WORKSPACE);
                self.report.paths_generalized += count as u32;
            }
        }
        for pattern in &self.machines {
            let count = pattern.find_iter(&value).count();
            if count > 0 {
                value = pattern.replace_all(&value, MACHINE).into_owned();
                self.report.machines_removed += count as u32;
            }
        }
        let mut found = Vec::new();
        value = assignment()
            .replace_all(&value, |captures: &regex::Captures<'_>| {
                let name = &captures[1];
                if !secret_name(name) {
                    return captures[0].to_string();
                }
                found.push(name.to_string());
                format!(
                    "{name}{}{}{SECRET_PLACEHOLDER} {name}]{}",
                    &captures[2], &captures[3], &captures[3]
                )
            })
            .into_owned();
        for name in found {
            self.report.secret_values_removed += 1;
            self.ask(&name, at);
        }
        let mut redacted = 0u32;
        value = long_token()
            .replace_all(&value, |captures: &regex::Captures<'_>| {
                let token = &captures[0];
                if token.contains(SECRET_PLACEHOLDER) {
                    return token.to_string();
                }
                let clean = codeterm_util::redact::text(token);
                if clean != token {
                    redacted += 1;
                }
                clean
            })
            .into_owned();
        self.report.secret_values_removed += redacted;
        value
    }

    fn ask(&mut self, name: &str, at: &str) {
        if self.secrets.insert(name.to_string()) {
            self.report.secret_names.push(name.to_string());
            self.asks.push(PackageSecret {
                name: name.to_string(),
                purpose: format!("Found in {at}; bind your own value at start"),
                required: None,
            });
        }
    }

    fn option(&mut self, value: &mut Option<String>, at: &str) {
        if let Some(text) = value.as_mut() {
            *text = self.text(text, at);
        }
    }

    /// Adds the secret asks found so far to `package`, keeping names it already declares.
    fn finish(&mut self, package: &mut Package) {
        for ask in self.asks.drain(..) {
            if package
                .document
                .secrets
                .iter()
                .all(|secret| secret.name != ask.name)
            {
                self.report.secret_asks_added += 1;
                package.document.secrets.push(ask);
            }
        }
        for secret in &package.document.secrets {
            if !self.report.secret_names.contains(&secret.name) {
                self.report.secret_names.push(secret.name.clone());
            }
        }
        self.report.secret_names.sort();
    }
}

/// Applies the projection to every text a package carries: instructions, files, memory,
/// skills and automation. Authors, license and links are intended and kept as they are.
pub fn scrub_package(package: &mut Package, context: &ScrubContext) -> ScrubReport {
    let mut scrubber = Scrubber::new(context);
    let document = &mut package.document;
    document.description = scrubber.text(&document.description, "description");
    scrubber.option(&mut document.title, "title");
    for when in document.when.iter_mut() {
        *when = scrubber.text(when, "when");
    }
    for (key, input) in document.inputs.iter_mut() {
        let at = format!("input {key}");
        match input {
            PackageInput::Short(description) => *description = scrubber.text(description, &at),
            PackageInput::Full(spec) => {
                spec.description = scrubber.text(&spec.description, &at);
                scrubber.option(&mut spec.default, &at);
            }
        }
    }
    let mut roles: Vec<(String, &mut codeterm_team_package::TeamRole)> = Vec::new();
    let managers = &mut document.managers;
    roles.push(("orchestrator".into(), &mut managers.orchestrator));
    for (name, slot) in [
        ("planner", managers.planner.as_mut()),
        ("cartographer", managers.cartographer.as_mut()),
        ("curator", managers.curator.as_mut()),
    ] {
        if let Some(role) = slot {
            roles.push((name.into(), role));
        }
    }
    for (name, role) in document.workers.iter_mut() {
        roles.push((name.to_string(), role));
    }
    for (name, role) in roles {
        let at = format!("the {name} instructions");
        scrubber.option(&mut role.instructions, &at);
        scrubber.option(&mut role.description, &at);
    }
    for file in document.files.iter_mut() {
        let at = format!("file {}", file.path);
        scrubber.option(&mut file.content, &at);
        scrubber.option(&mut file.description, &at);
    }
    for memory in document.memory.iter_mut() {
        let at = format!("memory \"{}\"", memory.title);
        memory.title = scrubber.text(&memory.title, &at);
        memory.body = scrubber.text(&memory.body, &at);
    }
    for skill in document.skills.iter_mut() {
        if let PackageSkill::Inline(inline) = skill {
            let at = format!("skill {}", inline.name);
            inline.description = scrubber.text(&inline.description, &at);
            inline.content = scrubber.text(&inline.content, &at);
            for file in inline.files.iter_mut() {
                file.content = scrubber.text(&file.content, &at);
            }
        }
    }
    for automation in document.automation.iter_mut() {
        match automation {
            PackageAutomation::Inline {
                name,
                description,
                content,
            } => {
                let at = format!("automation {name}");
                *content = scrubber.text(content, &at);
                scrubber.option(description, &at);
            }
            PackageAutomation::Path { path, description } => {
                let at = format!("automation {path}");
                scrubber.option(description, &at);
            }
        }
    }
    let paths: Vec<String> = package.entries.keys().cloned().collect();
    for path in paths {
        let Some(data) = package.entries.get(&path) else {
            continue;
        };
        let Ok(text) = std::str::from_utf8(data) else {
            continue;
        };
        let clean = scrubber.text(text, &format!("file {path}"));
        package.entries.insert(path, clean.into_bytes());
    }
    scrubber.finish(package);
    scrubber.report
}
