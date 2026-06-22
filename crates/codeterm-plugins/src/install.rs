use super::*;

pub(super) fn install_command(
    id: Option<String>,
    from_dir: Option<String>,
    id_override: Option<String>,
    channel: Option<String>,
) -> Result<serde_json::Value, String> {
    if let Some(dir) = from_dir {
        let source = absolutize(Path::new(&dir))?;
        let install_id = id_override
            .or(id)
            .or_else(|| manifest_id(&source).ok())
            .unwrap_or_else(|| {
                source
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("plugin")
                    .to_string()
            });
        let version = manifest_version(&source).ok().flatten();
        let record = InstallRecord {
            id: install_id.clone(),
            version: version.clone(),
            source: InstallSource::Dir {
                path: source.to_string_lossy().to_string(),
            },
            installed_at: now_rfc3339(),
            last_change: Some(last_change_for_install(version, None)),
        };
        if let Err(reason) = compat_check(manifest_min_codeterm(&source).as_deref()) {
            return Err(format!("plugin '{install_id}' {reason}"));
        }
        let before = manifest_version(&plugin_dir(&install_id)).ok().flatten();
        let result = install_from_dir(&source, &install_id, Some(&record));
        audit_plugin_result(
            "install",
            Some(&install_id),
            None,
            before,
            record.version.clone(),
            &result,
        );
        result?;
        let reload_required = manifest_requires_reload(&plugin_dir(&install_id));
        return Ok(serde_json::json!({
            "ok": true,
            "message": install_message(
                &format!("Installed plugin '{}' from {}", install_id, source.display()),
                &install_id,
                reload_required,
            ),
            "id": install_id,
            "path": plugin_dir(&install_id).display().to_string(),
            "reload_required": reload_required,
        }));
    }

    let id = id.ok_or("install requires <id> or --from-dir <path>")?;
    let mut warnings = Vec::new();
    let candidate = resolve_channel_install_candidate(&id, channel.as_deref(), &mut warnings)?;
    let source_dir = candidate.channel_path.join(&candidate.plugin_path);
    let channel_name = candidate.channel_name.clone();
    let version = candidate.version;
    let record = InstallRecord {
        id: id.clone(),
        version: version.clone(),
        source: InstallSource::Channel {
            channel: channel_name.clone(),
            path: candidate.plugin_path,
        },
        installed_at: now_rfc3339(),
        last_change: Some(last_change_for_install(version, Some(channel_name.clone()))),
    };
    if let Err(reason) = compat_check(manifest_min_codeterm(&source_dir).as_deref()) {
        return Err(format!("plugin '{id}' {reason}"));
    }
    let before = manifest_version(&plugin_dir(&id)).ok().flatten();
    let result = install_from_dir(&source_dir, &id, Some(&record));
    audit_plugin_result(
        "install",
        Some(&id),
        Some(&channel_name),
        before,
        record.version.clone(),
        &result,
    );
    result?;
    let reload_required = manifest_requires_reload(&plugin_dir(&id));
    Ok(serde_json::json!({
        "ok": true,
        "message": install_message(
            &format!("Installed plugin '{}' from channel", id),
            &id,
            reload_required,
        ),
        "id": id,
        "path": plugin_dir(&id).display().to_string(),
        "reload_required": reload_required,
        "warnings": warnings,
    }))
}

fn resolve_channel_install_candidate(
    id: &str,
    channel: Option<&str>,
    warnings: &mut Vec<String>,
) -> Result<ChannelCandidate, String> {
    if let Some(channel) = channel {
        refresh_channel_for_install(channel, warnings);
        return find_channel_plugin(id, Some(channel));
    }

    let cached = find_channel_plugin(id, None)?;
    let channel_name = cached.channel_name.clone();
    if refresh_channel_for_install(&channel_name, warnings) {
        return find_channel_plugin(id, Some(&channel_name));
    }
    Ok(cached)
}

fn refresh_channel_for_install(channel: &str, warnings: &mut Vec<String>) -> bool {
    match refresh_channel(Some(channel)) {
        Ok(()) => true,
        Err(err) => {
            warnings.push(format!(
                "refresh channel '{channel}' failed: {err}; using cached channel"
            ));
            false
        }
    }
}

/// Build the agent-readable install success message: the reload hint when a
/// plugin can't hot-activate (§1.8), plus a configuration next-step so the agent
/// that ran the install offers to configure it instead of leaving knobs unset.
/// `base` already says "Installed plugin '<id>' …", so we append only the
/// continuation sentence — never restate "Installed <id>" (m5).
fn install_message(base: &str, id: &str, reload_required: bool) -> String {
    let mut msg = base.to_string();
    if reload_required {
        msg.push_str(" — reload CodeTerm to activate");
    }
    msg.push_str(&format!(
        ". To configure it: run `codeterm plugin get {id}` to see options, \
         ask the user what they want, then `codeterm plugin config {id} --set key=value`."
    ));
    msg
}

pub(super) fn remove_installed(
    id: &str,
    force: bool,
    remove_data: bool,
) -> Result<serde_json::Value, String> {
    if is_reserved_system_id(id) && !force {
        return Err(format!(
            "'{id}' is a system plugin id; pass --force to remove its user-installed folder"
        ));
    }
    if remove_data && is_reserved_data_id(id) {
        return Err(format!("'{id}' is a reserved plugin data id"));
    }
    let dir = plugin_dir(id);
    if !dir.exists() {
        return Err(format!("plugin '{id}' is not installed"));
    }
    let data_dirs = if remove_data {
        removable_data_paths(id, &dir)?
    } else {
        Vec::new()
    };
    let before = manifest_version(&dir).ok().flatten();
    let result = fs::remove_dir_all(&dir).map_err(|e| format!("remove {}: {e}", dir.display()));
    audit_plugin_result("remove", Some(id), None, before, None, &result);
    result?;
    for data_dir in data_dirs {
        if data_dir.exists() {
            fs::remove_dir_all(&data_dir)
                .map_err(|e| format!("remove data {}: {e}", data_dir.display()))?;
        }
    }
    Ok(serde_json::json!({
        "ok": true,
        "message": format!("Removed plugin '{}'", id),
    }))
}

fn is_reserved_data_id(id: &str) -> bool {
    matches!(id, "plugins" | "channels" | "bin" | "logs")
}

fn removable_data_paths(id: &str, plugin_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths = vec![safe_data_path(&data_root().join(id))?];
    let manifest = read_plugin_manifest(plugin_dir)?;
    for raw in manifest.data_paths {
        paths.push(safe_data_path(&expand_home(&raw)?)?);
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn expand_home(raw: &str) -> Result<PathBuf, String> {
    if raw == "~" {
        return Ok(data_home());
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return Ok(data_home().join(rest));
    }
    Ok(PathBuf::from(raw))
}

fn data_home() -> PathBuf {
    if std::env::var_os("CODETERM_DATA_DIR").is_some() {
        data_root()
    } else {
        dirs::home_dir().unwrap_or_else(data_root)
    }
}

fn safe_data_path(path: &Path) -> Result<PathBuf, String> {
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(format!(
            "plugin data path contains parent segment: {}",
            path.display()
        ));
    }
    let canonical = canonicalize_existing_or_parent(path)?;
    let home = data_home()
        .canonicalize()
        .map_err(|e| format!("canonicalize home {}: {e}", data_home().display()))?;
    if !canonical.starts_with(&home) {
        return Err(format!(
            "plugin data path is outside home: {}",
            canonical.display()
        ));
    }
    Ok(canonical)
}

fn canonicalize_existing_or_parent(path: &Path) -> Result<PathBuf, String> {
    if path.exists() {
        return path
            .canonicalize()
            .map_err(|e| format!("canonicalize plugin data path {}: {e}", path.display()));
    }
    let parent = path.parent().ok_or_else(|| {
        format!(
            "plugin data path has no parent to canonicalize: {}",
            path.display()
        )
    })?;
    let name = path.file_name().ok_or_else(|| {
        format!(
            "plugin data path has no file name to canonicalize: {}",
            path.display()
        )
    })?;
    let parent = parent.canonicalize().map_err(|e| {
        format!(
            "canonicalize plugin data path parent {}: {e}",
            parent.display()
        )
    })?;
    Ok(parent.join(name))
}
pub(super) fn install_from_dir(
    source: &Path,
    id: &str,
    record: Option<&InstallRecord>,
) -> Result<(), String> {
    if !source.join("plugin.json").exists() {
        return Err(format!("{} has no plugin.json", source.display()));
    }
    let dest = plugin_dir(id);
    let tmp = dest.with_extension("tmp-install");
    if tmp.exists() {
        fs::remove_dir_all(&tmp).map_err(|e| format!("clear {}: {e}", tmp.display()))?;
    }
    copy_dir_filtered(source, &tmp)?;
    if let Some(record) = record {
        let body = serde_json::to_string_pretty(record).map_err(|e| e.to_string())?;
        fs::write(tmp.join(".codeterm-install.json"), body).map_err(|e| e.to_string())?;
    }
    if dest.exists() {
        fs::remove_dir_all(&dest).map_err(|e| format!("replace {}: {e}", dest.display()))?;
    }
    fs::rename(&tmp, &dest).map_err(|e| format!("install {}: {e}", dest.display()))?;
    Ok(())
}

pub(super) fn copy_dir_filtered(source: &Path, dest: &Path) -> Result<(), String> {
    let source = absolutize(source)?;
    if !source.is_dir() {
        return Err(format!("{} is not a directory", source.display()));
    }
    fs::create_dir_all(dest).map_err(|e| format!("mkdir {}: {e}", dest.display()))?;
    copy_dir_inner(&source, dest, &source)
}

pub(super) fn copy_dir_inner(source: &Path, dest: &Path, root: &Path) -> Result<(), String> {
    for entry in fs::read_dir(source).map_err(|e| format!("read_dir {}: {e}", source.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if should_skip(&name) {
            continue;
        }
        let rel = path.strip_prefix(root).map_err(|e| e.to_string())?;
        let out = dest.join(rel);
        let ft = entry.file_type().map_err(|e| e.to_string())?;
        if ft.is_dir() {
            fs::create_dir_all(&out).map_err(|e| format!("mkdir {}: {e}", out.display()))?;
            copy_dir_inner(&path, dest, root)?;
        } else if ft.is_file() {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
            }
            fs::copy(&path, &out).map_err(|e| format!("copy {}: {e}", path.display()))?;
        }
    }
    Ok(())
}

pub(super) fn should_skip(name: &str) -> bool {
    matches!(
        name,
        ".git" | "node_modules" | "target" | ".DS_Store" | "Thumbs.db" | "desktop.ini"
    )
}

pub(super) fn read_plugin_manifest(dir: &Path) -> Result<PluginManifest, String> {
    let text = fs::read_to_string(dir.join("plugin.json"))
        .map_err(|e| format!("read {}: {e}", dir.join("plugin.json").display()))?;
    serde_json::from_str(&text).map_err(|e| format!("parse plugin.json: {e}"))
}

pub(super) fn read_channel_manifest(dir: &Path) -> Result<ChannelManifest, String> {
    let text = fs::read_to_string(dir.join("channel.json"))
        .map_err(|e| format!("read {}: {e}", dir.join("channel.json").display()))?;
    serde_json::from_str(&text).map_err(|e| format!("parse channel.json: {e}"))
}

pub(super) fn manifest_id(dir: &Path) -> Result<String, String> {
    Ok(read_plugin_manifest(dir)?.id)
}

pub(super) fn manifest_version(dir: &Path) -> Result<Option<String>, String> {
    Ok(read_plugin_manifest(dir)?.version)
}

/// Whether the installed plugin declares `requiresReload` (§1.8). Best-effort:
/// a missing/unparseable manifest reads as `false`.
pub(super) fn manifest_requires_reload(dir: &Path) -> bool {
    read_plugin_manifest(dir)
        .map(|m| m.requires_reload)
        .unwrap_or(false)
}

pub(super) fn manifest_hash(dir: &Path) -> Result<String, String> {
    let path = dir.join("plugin.json");
    let bytes = fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}
