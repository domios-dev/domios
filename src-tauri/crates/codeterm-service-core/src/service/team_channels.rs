use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use codeterm_plugins::{
    channel_kind, fetch_channel, refresh_fetched_channel, validate_channel_name, ChannelKind,
};
pub use codeterm_team_package::contracts::{
    SkipReason, SkippedTeam, TeamChannelAction, TeamChannelKind, TeamChannelRow,
};
use codeterm_team_package::{check_folder, CheckOptions, Package};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

mod refusal;
pub use refusal::TeamChannelRefusal;

fn kind(url: &str) -> TeamChannelKind {
    match channel_kind(url) {
        ChannelKind::Local => TeamChannelKind::Local,
        ChannelKind::Git => TeamChannelKind::Git,
        ChannelKind::Archive => TeamChannelKind::Archive,
    }
}

pub const OFFICIAL: &str = "domios-teams";
/// The official channel's one name everywhere; its published index does not rename it.
pub const OFFICIAL_DISPLAY_NAME: &str = "Domios teams";
pub const OFFICIAL_URL: &str = "https://github.com/domios-dev/domios-teams/releases/download/domios-teams-latest/domios-teams.tar.gz";

pub fn command(
    svc: &super::ServiceContext,
    action: TeamChannelAction,
) -> Result<serde_json::Value, TeamChannelRefusal> {
    let store = TeamChannelStore::global();
    let changed = !matches!(action, TeamChannelAction::List);
    let list = || store.list().map_err(TeamChannelRefusal::failed);
    let rows = match action {
        TeamChannelAction::List => list()?,
        TeamChannelAction::Add { url, name } => {
            store.add(&url, name.as_deref())?;
            list()?
        }
        TeamChannelAction::Refresh { name } => store.refresh(name.as_deref())?,
        TeamChannelAction::Remove { name } => {
            store.remove(&name)?;
            list()?
        }
    };
    if changed {
        let _ = super::team_template_recall::reindex(svc);
    }
    Ok(serde_json::json!({ "channels": rows }))
}

#[derive(Default, Serialize, Deserialize)]
struct Registry {
    channels: BTreeMap<String, TeamChannelRow>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Index {
    schema_version: u32,
    name: String,
    display_name: String,
    description: String,
    teams: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    name: String,
    path: String,
    version: String,
    min_domios: String,
}

pub struct TeamChannelStore {
    root: PathBuf,
    writes: Mutex<()>,
    startup: Mutex<bool>,
    official_url: String,
    version: String,
    packages: Mutex<Option<Vec<(TeamChannelRow, Package)>>>,
}

impl TeamChannelStore {
    pub fn new(root: PathBuf, version: &str) -> Self {
        Self::with_url(
            root,
            std::env::var("CODETERM_TEAM_CHANNEL_URL")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| OFFICIAL_URL.into()),
            version,
        )
    }

    fn with_url(root: PathBuf, official_url: String, version: &str) -> Self {
        Self {
            root,
            writes: Mutex::new(()),
            startup: Mutex::new(false),
            official_url,
            version: version.into(),
            packages: Mutex::new(None),
        }
    }

    pub fn global() -> &'static Self {
        static STORE: OnceLock<TeamChannelStore> = OnceLock::new();
        STORE.get_or_init(|| Self::new(crate::paths::data_dir(), codeterm_build_info::version()))
    }

    fn registry_path(&self) -> PathBuf {
        self.root.join("team-channels.json")
    }

    fn read(&self) -> Result<Registry, String> {
        match std::fs::read(self.registry_path()) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| error.to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Registry::default()),
            Err(error) => Err(error.to_string()),
        }
    }

    fn write(&self, registry: &Registry) -> Result<(), String> {
        std::fs::create_dir_all(&self.root).map_err(|error| error.to_string())?;
        let bytes = serde_json::to_vec_pretty(registry).map_err(|error| error.to_string())?;
        codeterm_util::atomic_write::write_atomic(&self.registry_path(), &bytes)
            .map_err(|error| error.to_string())
    }

    fn official(&self) -> TeamChannelRow {
        let url = self.official_url.clone();
        TeamChannelRow {
            name: OFFICIAL.into(),
            display_name: OFFICIAL_DISPLAY_NAME.into(),
            description: String::new(),
            kind: kind(&url),
            url,
            path: self
                .root
                .join("team-channels")
                .join(OFFICIAL)
                .display()
                .to_string(),
            official: true,
            last_refresh: None,
            skipped_entries: Vec::new(),
            usable_teams: None,
        }
    }

    pub fn list(&self) -> Result<Vec<TeamChannelRow>, String> {
        let mut registry = self.read()?;
        let mut official = self.official();
        if let Some(previous) = registry.channels.remove(OFFICIAL) {
            official.description = previous.description;
            official.last_refresh = previous.last_refresh;
            official.skipped_entries = previous.skipped_entries;
            official.usable_teams = previous.usable_teams;
        }
        let mut rows = vec![official];
        rows.extend(registry.channels.into_values().map(|mut row| {
            row.official = false;
            row
        }));
        Ok(rows)
    }

    pub fn add(&self, url: &str, name: Option<&str>) -> Result<TeamChannelRow, TeamChannelRefusal> {
        let _ = self.packages();
        let _guard = self.writes.lock();
        let invalid_name = |error: String| TeamChannelRefusal::new("invalid_name", error);
        let mut registry = self.read().map_err(TeamChannelRefusal::failed)?;
        if let Some(name) = name {
            validate_channel_name(name).map_err(invalid_name)?;
        }
        let channels = self.root.join("team-channels");
        let temporary = channels.join(format!("staging-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&channels)
            .map_err(|error| TeamChannelRefusal::failed(error.to_string()))?;
        let result = (|| {
            fetch_channel("team-channel", url, &temporary)
                .map_err(|failure| TeamChannelRefusal::fetch(&failure, &kind(url)))?;
            let (index, packages, skipped) = check_index(&temporary, false, &self.version)
                .map_err(|error| TeamChannelRefusal::new("not_a_channel", error))?;
            let name = name.unwrap_or(&index.name);
            validate_channel_name(name).map_err(invalid_name)?;
            let path = channels.join(name);
            if name == OFFICIAL || registry.channels.contains_key(name) || path.exists() {
                return Err(TeamChannelRefusal::new(
                    "already_registered",
                    format!("A channel named '{name}' is already added"),
                ));
            }
            std::fs::rename(&temporary, &path)
                .map_err(|error| TeamChannelRefusal::failed(error.to_string()))?;
            let row = TeamChannelRow {
                name: name.into(),
                display_name: index.display_name,
                description: index.description,
                url: url.into(),
                path: path.display().to_string(),
                kind: kind(url),
                official: false,
                last_refresh: Some("ok".into()),
                skipped_entries: skipped,
                usable_teams: Some(packages.len() as u32),
            };
            registry.channels.insert(name.into(), row.clone());
            self.write(&registry).map_err(TeamChannelRefusal::failed)?;
            *self.packages.lock() = None;
            Ok(row)
        })();
        let _ = std::fs::remove_dir_all(&temporary);
        result
    }

    pub fn refresh(&self, name: Option<&str>) -> Result<Vec<TeamChannelRow>, TeamChannelRefusal> {
        let _ = self.packages();
        let _guard = self.writes.lock();
        std::fs::create_dir_all(self.root.join("team-channels"))
            .map_err(|error| TeamChannelRefusal::failed(error.to_string()))?;
        let mut registry = self.read().map_err(TeamChannelRefusal::failed)?;
        let rows: Vec<_> = self
            .list()
            .map_err(TeamChannelRefusal::failed)?
            .into_iter()
            .filter(|row| name.is_none_or(|name| name == row.name))
            .collect();
        if rows.is_empty() {
            return Err(not_registered());
        }
        for mut row in rows {
            let previous = if row.official {
                check_index(Path::new(&row.path), true, &self.version)
                    .ok()
                    .map(|(_, packages, _)| packages)
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            let mut checked = None;
            let result = refresh_fetched_channel(
                &row.name,
                &row.url,
                Path::new(&row.path),
                |path| {
                    let candidate = check_index(path, row.official, &self.version)?;
                    for package in &candidate.1 {
                        if let Some(old) = previous
                            .iter()
                            .find(|old| old.document.name == package.document.name)
                        {
                            if matches!(
                                (semver::Version::parse(&package.document.version), semver::Version::parse(&old.document.version)),
                                (Ok(next), Ok(previous)) if next.cmp_precedence(&previous).is_lt()
                            ) {
                                return Err(format!(
                                    "official package {} would downgrade {} to {}",
                                    package.document.name,
                                    old.document.version,
                                    package.document.version
                                ));
                            }
                        }
                    }
                    checked = Some(candidate);
                    Ok(())
                },
            );
            row.last_refresh = Some(match result {
                Ok(()) => {
                    if let Some((index, packages, skipped)) = checked {
                        row.display_name = index.display_name;
                        row.description = index.description;
                        row.skipped_entries = skipped;
                        row.usable_teams = Some(packages.len() as u32);
                    }
                    "ok".into()
                }
                Err(failure) => TeamChannelRefusal::fetch(&failure, &row.kind).message,
            });
            registry.channels.insert(row.name.clone(), row);
        }
        self.write(&registry).map_err(TeamChannelRefusal::failed)?;
        *self.packages.lock() = None;
        self.list().map_err(TeamChannelRefusal::failed)
    }

    pub fn refresh_official_once(&self) {
        let Some(mut started) = self.startup.try_lock() else {
            return;
        };
        if *started {
            return;
        }
        *started = true;
        let _ = self.refresh(Some(OFFICIAL));
    }

    pub fn remove(&self, name: &str) -> Result<(), TeamChannelRefusal> {
        validate_channel_name(name)
            .map_err(|error| TeamChannelRefusal::new("invalid_name", error))?;
        if name == OFFICIAL {
            return Err(TeamChannelRefusal::new(
                "official",
                "The official team channel can't be removed",
            ));
        }
        let _ = self.packages();
        let _guard = self.writes.lock();
        let mut registry = self.read().map_err(TeamChannelRefusal::failed)?;
        registry.channels.remove(name).ok_or_else(not_registered)?;
        self.write(&registry).map_err(TeamChannelRefusal::failed)?;
        *self.packages.lock() = None;
        let path = self.root.join("team-channels").join(name);
        if path.exists() {
            std::fs::remove_dir_all(path)
                .map_err(|error| TeamChannelRefusal::failed(error.to_string()))?;
        }
        Ok(())
    }

    pub fn packages(&self) -> Vec<(TeamChannelRow, Package)> {
        let mut cached = self.packages.lock();
        cached
            .get_or_insert_with(|| {
                let mut result = Vec::new();
                for row in self.list().unwrap_or_default() {
                    if let Ok((_, packages, _)) =
                        check_index(Path::new(&row.path), row.official, &self.version)
                    {
                        result.extend(packages.into_iter().map(|package| (row.clone(), package)));
                    }
                }
                result
            })
            .clone()
    }
}

fn not_registered() -> TeamChannelRefusal {
    TeamChannelRefusal::new("not_registered", "No team channel by that name")
}

/// Index refusals are plain words for the owner; OS-localized io detail stays in the log.
fn check_index(
    root: &Path,
    official: bool,
    version: &str,
) -> Result<(Index, Vec<Package>, Vec<SkippedTeam>), String> {
    use std::io::Read;
    let unreadable = |error: std::io::Error| {
        log::warn!("team channel index unreadable: {error}");
        "The channel.json can't be read".to_string()
    };
    let file = match std::fs::File::open(root.join("channel.json")) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err("No channel.json here, so this isn't a team channel".into());
        }
        file => file.map_err(unreadable)?,
    };
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(unreadable)?;
    if bytes.len() > 1024 * 1024 {
        return Err("The channel.json is larger than 1 MiB".into());
    }
    let index: Index = serde_json::from_slice(&bytes)
        .map_err(|error| format!("The channel.json isn't valid: {error}"))?;
    if index.schema_version != 1 {
        return Err("The channel.json uses a format this Domios doesn't read".into());
    }
    validate_channel_name(&index.name)?;
    if index.teams.len() > 512 {
        return Err("team channel exceeds 512 entries".into());
    }
    let mut packages = Vec::new();
    let mut skipped = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    for entry in &index.teams {
        let result = check_entry(root, entry, official, version).and_then(|package| {
            if !names.insert(entry.name.clone()) {
                return Err((SkipReason::DuplicateName, "duplicate team name".into()));
            }
            Ok(package)
        });
        match result {
            Ok(package) => packages.push(package),
            Err((code, reason)) => skipped.push(SkippedTeam {
                name: entry.name.clone(),
                code,
                reason,
                required_domios: (code == SkipReason::RequiresNewerDomios)
                    .then(|| entry.min_domios.clone()),
            }),
        }
    }
    Ok((index, packages, skipped))
}

type Skip = (SkipReason, String);

fn skip(code: SkipReason, reason: impl Into<String>) -> Skip {
    (code, reason.into())
}

fn check_entry(root: &Path, entry: &Entry, official: bool, version: &str) -> Result<Package, Skip> {
    if !official && entry.name.starts_with("dev.domios/") {
        return Err(skip(
            SkipReason::ReservedName,
            "dev.domios is owned by builtins",
        ));
    }
    let path = Path::new(&entry.path);
    if entry.path.contains('\\')
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        || entry.path.is_empty()
    {
        return Err(skip(
            SkipReason::InvalidPath,
            "package path must be contained and relative",
        ));
    }
    let root = root
        .canonicalize()
        .map_err(|error| skip(SkipReason::Other, error.to_string()))?;
    let path = root
        .join(path)
        .canonicalize()
        .map_err(|error| skip(SkipReason::MissingPackage, error.to_string()))?;
    if !path.starts_with(&root) {
        return Err(skip(
            SkipReason::InvalidPath,
            "package path escapes channel",
        ));
    }
    let required = semver::Version::parse(&entry.min_domios)
        .map_err(|_| skip(SkipReason::InvalidIndexEntry, "invalid minDomios"))?;
    let current = semver::Version::parse(version)
        .map_err(|_| skip(SkipReason::Other, "invalid host version"))?;
    if required.cmp_precedence(&current).is_gt() {
        return Err(skip(
            SkipReason::RequiresNewerDomios,
            format!("requires Domios {}", entry.min_domios),
        ));
    }
    semver::Version::parse(&entry.version)
        .map_err(|_| skip(SkipReason::InvalidIndexEntry, "invalid declared version"))?;
    let checked = check_folder(&path, &CheckOptions::default());
    if !checked.report.valid {
        return Err(skip(
            SkipReason::InvalidPackage,
            format!(
                "invalid package: {:?}",
                checked.report.errors().collect::<Vec<_>>()
            ),
        ));
    }
    let package = checked
        .package
        .ok_or_else(|| skip(SkipReason::InvalidPackage, "missing checked package"))?;
    if package.document.name != entry.name || package.document.version != entry.version {
        return Err(skip(
            SkipReason::IndexMismatch,
            "package name/version mismatches index",
        ));
    }
    super::team_template_start::host_policy(&package)
        .map_err(|error| skip(SkipReason::HostPolicy, format!("host policy: {error:?}")))?;
    Ok(package)
}

#[cfg(test)]
mod tests;
