use codeterm_plugin::agent_runtime::{RuntimeConfigOption, RuntimeLaunchProfile, RuntimeSettings};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
    sync::LazyLock,
};

const MAX_BYTES: u64 = 256 * 1024;
const MAX_PENDING: usize = 256;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Snapshot {
    declaration_sha256: String,
    config_options: Vec<RuntimeConfigOption>,
}

#[derive(Default)]
struct Writes {
    running: bool,
    pending: BTreeMap<PathBuf, Snapshot>,
    latest: BTreeMap<PathBuf, Snapshot>,
}
static WRITES: LazyLock<Mutex<Writes>> = LazyLock::new(|| Mutex::new(Writes::default()));
static FILES: LazyLock<Mutex<()>> = LazyLock::new(Default::default);

fn path(root: &Path, id: &str) -> Option<PathBuf> {
    if id.is_empty()
        || id.len() > 128
        || codeterm_plugin::agent_runtime::install_validation::reserved_name(id)
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return None;
    }
    Some(root.join(format!("{id}.options.json")))
}

pub(super) fn read(
    root: &Path,
    id: &str,
    profile: &RuntimeLaunchProfile,
) -> Vec<RuntimeConfigOption> {
    let Some(path) = path(root, id) else {
        return Vec::new();
    };
    let pending = WRITES.lock().latest.get(&path).cloned();
    let snapshot = pending.or_else(|| {
        let file = std::fs::File::open(path).ok()?;
        if file.metadata().ok()?.len() > MAX_BYTES {
            return None;
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1).read_to_end(&mut bytes).ok()?;
        if bytes.len() as u64 > MAX_BYTES {
            return None;
        }
        serde_json::from_slice::<Snapshot>(&bytes).ok()
    });
    let Some(snapshot) = snapshot else {
        return Vec::new();
    };
    if !matches!(
        super::launch_profile_install::bound_by(profile, &snapshot.declaration_sha256),
        Ok(Some(_))
    ) {
        return Vec::new();
    }
    let settings = RuntimeSettings {
        config_options: Some(snapshot.config_options.clone()),
        ..Default::default()
    };
    if codeterm_plugin::agent_runtime::validate_runtime_settings(&settings).is_err() {
        return Vec::new();
    }
    snapshot
        .config_options
        .into_iter()
        .filter(selectable)
        .collect()
}

fn selectable(option: &RuntimeConfigOption) -> bool {
    matches!(option.category.as_deref(), Some("model" | "thought_level"))
}

pub(crate) fn resolve(
    options: &[RuntimeConfigOption],
    category: &str,
    requested: &str,
) -> Result<String, String> {
    let choices = options
        .iter()
        .filter(|option| option.category.as_deref() == Some(category))
        .flat_map(|option| &option.options)
        .collect::<Vec<_>>();
    if let Some(choice) = choices.iter().find(|choice| choice.value == requested) {
        return Ok(choice.value.clone());
    }
    let named = choices
        .iter()
        .filter(|choice| choice.name == requested)
        .collect::<Vec<_>>();
    if named.len() == 1 {
        return Ok(named[0].value.clone());
    }
    let offered = choices
        .iter()
        .map(|choice| format!("{} (value: {})", choice.name, choice.value))
        .collect::<Vec<_>>()
        .join(", ");
    let title = if category == "model" {
        "model"
    } else {
        "reasoning effort"
    };
    Err(format!(
        "Requested {title} '{requested}' is not uniquely offered; offered choices: {}",
        if offered.is_empty() {
            "none; start the profile without explicit selections to discover its catalogue"
        } else {
            &offered
        }
    ))
}

impl crate::service::ServiceContext {
    pub(crate) fn resolve_spawn_profile_options(
        &self,
        provider: &str,
        params: &mut crate::service::SpawnAgentParams,
    ) -> Result<(), String> {
        let Some(id) = params.launch_profile.as_deref() else {
            return Ok(());
        };
        let profile = super::launch_profile_install::declaration(self, provider, id)?;
        let options = read(
            &super::launch_profile_install::profile_root(provider)?,
            id,
            &profile,
        );
        let model = params
            .model
            .as_deref()
            .map(|value| resolve(&options, "model", value))
            .transpose()?;
        let effort = params
            .reasoning_effort
            .as_deref()
            .map(|value| resolve(&options, "thought_level", value))
            .transpose()?;
        params.model = model;
        params.reasoning_effort = effort;
        Ok(())
    }
}

fn snapshot(profile: &RuntimeLaunchProfile, settings: &RuntimeSettings) -> Option<Snapshot> {
    codeterm_plugin::agent_runtime::validate_runtime_settings(settings).ok()?;
    let snapshot = Snapshot {
        declaration_sha256: super::launch_profile_install::fingerprint(profile).ok()?,
        config_options: settings
            .config_options
            .as_ref()?
            .iter()
            .filter(|option| selectable(option))
            .cloned()
            .collect(),
    };
    (serde_json::to_vec(&snapshot).ok()?.len() as u64 <= MAX_BYTES).then_some(snapshot)
}

fn write(path: &Path, snapshot: &Snapshot) -> Result<(), String> {
    let bytes = serde_json::to_vec(snapshot).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Profile options exceed storage limit".into());
    }
    std::fs::create_dir_all(path.parent().ok_or("Missing profile directory")?)
        .map_err(|error| error.to_string())?;
    codeterm_util::atomic_write::write_atomic(path, bytes).map_err(|error| error.to_string())
}

pub(crate) fn record(
    svc: &crate::service::ServiceContext,
    provider: &str,
    id: &str,
    settings: &RuntimeSettings,
) {
    if settings.config_options.is_none() {
        return;
    }
    let Ok(profile) = super::launch_profile_install::declaration(svc, provider, id) else {
        return;
    };
    let Some(snapshot) = snapshot(&profile, settings) else {
        return;
    };
    let Some(path) = super::launch_profile_install::profile_root(provider)
        .ok()
        .and_then(|root| path(&root, id))
    else {
        return;
    };
    let mut writes = WRITES.lock();
    if writes.latest.get(&path) == Some(&snapshot) {
        return;
    }
    if writes.pending.len() >= MAX_PENDING && !writes.pending.contains_key(&path) {
        log::debug!("Profile option persistence queue is full");
        return;
    }
    if writes.latest.len() >= MAX_PENDING && !writes.latest.contains_key(&path) {
        writes.latest.pop_first();
    }
    writes.latest.insert(path.clone(), snapshot.clone());
    writes.pending.insert(path, snapshot);
    if writes.running {
        return;
    }
    writes.running = true;
    drop(writes);
    crate::async_runtime::spawn_blocking(|| loop {
        let _files = FILES.lock();
        let next = {
            let mut writes = WRITES.lock();
            match writes.pending.pop_first() {
                Some(next) => Some(next),
                None => {
                    writes.running = false;
                    None
                }
            }
        };
        let Some((path, snapshot)) = next else { break };
        if let Err(error) = write(&path, &snapshot) {
            log::debug!("Profile options could not be saved: {error}");
        }
    });
}

/// Drops this profile's remembered and queued options, then its file.
pub(super) fn forget(root: &Path, id: &str) -> Result<(), String> {
    let Some(path) = path(root, id) else {
        return Ok(());
    };
    {
        let mut writes = WRITES.lock();
        writes.pending.remove(&path);
        writes.latest.remove(&path);
    }
    // Waits out a write already taken from the queue, which would otherwise outlive the delete.
    let _files = FILES.lock();
    super::launch_profile_install::remove_file_if_present(&path)
}

#[cfg(test)]
pub(crate) fn seed_catalogue_for_test(
    svc: &crate::service::ServiceContext,
    provider: &str,
    id: &str,
    settings: serde_json::Value,
) {
    let profile = super::launch_profile_install::declaration(svc, provider, id).unwrap();
    let settings: RuntimeSettings = serde_json::from_value(settings).unwrap();
    let root = super::launch_profile_install::profile_root(provider).unwrap();
    WRITES.lock().latest.insert(
        path(&root, id).unwrap(),
        snapshot(&profile, &settings).unwrap(),
    );
}

#[cfg(test)]
mod tests;
