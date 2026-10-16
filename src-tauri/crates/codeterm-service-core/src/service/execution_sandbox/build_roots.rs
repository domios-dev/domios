use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(super) struct BuildRoots {
    pub writable_roots: Vec<String>,
    pub environment: BTreeMap<String, String>,
}

fn select(
    lane: &Path,
    home: &Path,
    platform: &str,
    env: &BTreeMap<String, String>,
) -> BTreeMap<String, PathBuf> {
    let supplied = |key: &str| {
        env.get(key)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    let cargo = supplied("CARGO_HOME").unwrap_or_else(|| home.join(".cargo"));
    let lanes = supplied("CODETERM_RUST_CACHE_ROOT")
        .map(|root| root.join("lanes"))
        .unwrap_or_else(|| cargo.join("codeterm-lanes"));
    let sccache = supplied("SCCACHE_DIR").unwrap_or_else(|| match platform {
        "windows" => supplied("LOCALAPPDATA")
            .unwrap_or_else(|| home.join("AppData/Local"))
            .join("Mozilla/sccache/cache"),
        "macos" => home.join("Library/Caches/Mozilla.sccache"),
        _ => supplied("XDG_CACHE_HOME")
            .unwrap_or_else(|| home.join(".cache"))
            .join("sccache"),
    });
    let store = supplied("npm_config_store_dir")
        .or_else(|| supplied("NPM_CONFIG_STORE_DIR"))
        .unwrap_or_else(|| lane.join(".codeterm-pnpm-store"));
    BTreeMap::from([
        ("CARGO_HOME".into(), cargo),
        ("CARGO_BUILD_BUILD_DIR".into(), lanes),
        ("SCCACHE_DIR".into(), sccache),
        ("npm_config_store_dir".into(), store),
        ("TMPDIR".into(), lane.join(".codeterm-tmp")),
    ])
}

fn resolve(
    selected: BTreeMap<String, PathBuf>,
    mut canonicalize: impl FnMut(&Path) -> Result<String, String>,
) -> Result<BuildRoots, String> {
    let mut environment = BTreeMap::new();
    let mut writable_roots = Vec::new();
    for (key, path) in selected {
        if !path.is_absolute() || path.parent().is_none() {
            return Err(format!(
                "Execution sandbox build root {key} must be an absolute non-root directory: {}",
                path.display()
            ));
        }
        let root = canonicalize(&path)?;
        if !writable_roots.contains(&root) {
            writable_roots.push(root.clone());
        }
        environment.insert(key, root);
    }
    let temp = environment
        .get("TMPDIR")
        .ok_or("Execution sandbox temp root missing")?
        .clone();
    environment.insert("TEMP".into(), temp.clone());
    environment.insert("TMP".into(), temp);
    Ok(BuildRoots {
        writable_roots,
        environment,
    })
}

pub(super) fn for_lane(lane: &str) -> Result<BuildRoots, String> {
    let lane = Path::new(lane);
    let home =
        dirs::home_dir().ok_or("Execution sandbox requires a home directory for build roots")?;
    let env = std::env::vars().collect();
    let resolved = resolve(select(lane, &home, std::env::consts::OS, &env), |path| {
        std::fs::create_dir_all(path).map_err(|error| {
            format!(
                "Cannot create sandbox build root {}: {error}",
                path.display()
            )
        })?;
        let canonical = path.canonicalize().map_err(|error| {
            format!(
                "Cannot resolve sandbox build root {}: {error}",
                path.display()
            )
        })?;
        if canonical.parent().is_none() {
            return Err(format!(
                "Sandbox build root resolves to a filesystem root: {}",
                path.display()
            ));
        }
        canonical
            .to_str()
            .map(codeterm_paths::strip_verbatim_prefix)
            .ok_or_else(|| "Sandbox build root is not Unicode".into())
    })?;
    if !Path::new(&resolved.environment["TMPDIR"]).starts_with(lane) {
        return Err(format!(
            "Sandbox temp root escaped lane: {}",
            resolved.environment["TMPDIR"]
        ));
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests;
