use super::{AgentRuntimeCapability, RuntimeLaunchProfile};
use parking_lot::Mutex;
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
    sync::OnceLock,
};

pub type Profiles = BTreeMap<String, RuntimeLaunchProfile>;
const MAX_BYTES: u64 = 2 * 1024 * 1024;
type Cache = BTreeMap<PathBuf, Result<Profiles, String>>;
fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

pub fn path(provider: &str) -> Result<PathBuf, String> {
    valid_name(provider)?;
    Ok(codeterm_paths::data_dir()
        .join("providers")
        .join(provider)
        .join("profiles/owner-profiles.json"))
}

pub fn valid_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 128
        || super::install_validation::reserved_name(name)
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(
            "Profile name must be one word of letters, digits, underscores or hyphens".into(),
        );
    }
    Ok(())
}

/// A profile's install receipt is `<name>.json` beside this store, so this name would be the store.
pub fn names_the_store(name: &str) -> bool {
    name.eq_ignore_ascii_case("owner-profiles")
}

pub fn read(path: &Path) -> Result<Profiles, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(_) => return Err("Owner profiles unavailable".into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Owner profiles unavailable")?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Owner profiles exceed size limit".into());
    }
    let mut profiles: Profiles =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid owner profiles")?;
    if profiles.len() > 120 {
        return Err("Too many owner profiles".into());
    }
    for (name, profile) in &mut profiles {
        valid_name(name)?;
        if !profile.owner_configured {
            return Err("Invalid owner profile authority".into());
        }
        if let Some(url) = &profile.icon_url {
            let provider_root = path
                .parent()
                .and_then(Path::parent)
                .and_then(Path::file_name)
                .map(|name| codeterm_paths::data_dir().join(name));
            profile.icon_svg = provider_root.and_then(|root| {
                crate::plugin_icon::registry::read_cached(&root.join("cache/icons"), url)
            });
        }
    }
    Ok(profiles)
}

pub fn load(provider: &str) -> Result<Profiles, String> {
    let path = path(provider)?;
    cache()
        .lock()
        .entry(path.clone())
        .or_insert_with(|| read(&path))
        .clone()
}

pub fn refresh_icon(provider: &str, url: &str, icon: &str) -> Result<bool, String> {
    refresh_icon_at(&path(provider)?, url, icon)
}

fn refresh_icon_at(path: &Path, url: &str, icon: &str) -> Result<bool, String> {
    let mut cache = cache().lock();
    let profiles = cache
        .entry(path.to_owned())
        .or_insert_with(|| read(path))
        .as_mut()
        .map_err(|error| error.clone())?;
    let mut changed = false;
    for profile in profiles.values_mut() {
        if profile.icon_url.as_deref() == Some(url) && profile.icon_svg.as_deref() != Some(icon) {
            profile.icon_svg = Some(icon.to_owned());
            changed = true;
        }
    }
    Ok(changed)
}

pub fn update<T>(
    provider: &str,
    change: impl FnOnce(&mut Profiles) -> Result<T, String>,
) -> Result<T, String> {
    let path = path(provider)?;
    update_at(&path, change)
}

fn update_at<T>(
    path: &Path,
    change: impl FnOnce(&mut Profiles) -> Result<T, String>,
) -> Result<T, String> {
    let mut cache = cache().lock();
    let mut profiles = cache
        .entry(path.to_owned())
        .or_insert_with(|| read(path))
        .clone()?;
    let result = change(&mut profiles)?;
    if profiles.len() > 120 {
        return Err("Too many owner profiles".into());
    }
    let mut declarations = profiles.clone();
    for profile in declarations.values_mut() {
        profile.icon_svg = None;
    }
    let bytes = serde_json::to_vec(&declarations).map_err(|_| "Invalid owner profiles")?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Owner profiles exceed size limit".into());
    }
    std::fs::create_dir_all(path.parent().ok_or("Profile directory unavailable")?)
        .map_err(|_| "Profile directory unavailable")?;
    codeterm_util::atomic_write::write_atomic(&path, bytes)
        .map_err(|_| "Owner profiles could not be saved")?;
    cache.insert(path.to_owned(), Ok(profiles));
    Ok(result)
}

pub fn overlay(
    provider: &str,
    capability: AgentRuntimeCapability,
) -> Result<AgentRuntimeCapability, String> {
    if capability.launch_profiles.is_none() {
        return Ok(capability);
    }
    overlay_with(capability, load(provider)?)
}

pub fn overlay_with(
    mut capability: AgentRuntimeCapability,
    owner: Profiles,
) -> Result<AgentRuntimeCapability, String> {
    if let Some(profiles) = &mut capability.launch_profiles {
        // An inherited bundle icon is resolved from the same sanitized asset its built-in declares.
        let bundle_icons: BTreeMap<String, String> = profiles
            .values()
            .filter_map(|profile| Some((profile.icon.clone()?, profile.icon_svg.clone()?)))
            .collect();
        for (id, mut profile) in owner {
            if profiles.contains_key(&id) {
                return Err("Owner profile collides with a built-in profile".into());
            }
            if profile.icon_svg.is_none() {
                profile.icon_svg = profile
                    .icon
                    .as_ref()
                    .and_then(|icon| bundle_icons.get(icon).cloned());
            }
            profiles.insert(id, profile);
        }
        // Resolved on every load, so existing installs gain it without rewriting their records.
        for profile in profiles.values_mut() {
            super::limits_source::resolve_registry_defaults(profile, &capability.registry_defaults);
        }
        capability
            .validate_declaration()
            .map_err(|_| "Invalid owner profile declaration")?;
    }
    Ok(capability)
}

#[cfg(test)]
mod tests;
