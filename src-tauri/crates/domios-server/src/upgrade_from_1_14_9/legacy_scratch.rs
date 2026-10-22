use super::*;

#[derive(Debug)]
struct Entry {
    name: PathBuf,
    kind: DestinationEntryKind,
    bytes: u64,
    empty_tree: bool,
}

fn recreatable(entries: &[Entry]) -> bool {
    entries.iter().all(|entry| {
        entry.kind == DestinationEntryKind::Directory
            && (entry.empty_tree || matches!(entry.name.to_str(), Some("logs" | "runtime")))
    })
}

fn tree_size(path: &Path) -> io::Result<(u64, bool)> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Ok((metadata.len(), false));
    }
    let mut bytes = 0u64;
    let mut empty = true;
    for child in std::fs::read_dir(path)? {
        let (size, child_empty) = tree_size(&child?.path())?;
        bytes = bytes.saturating_add(size);
        empty &= child_empty;
    }
    Ok((bytes, empty))
}

fn inventory(root: &Path) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let kind = entry_kind(&std::fs::symlink_metadata(entry.path())?);
        let (bytes, empty_tree) = tree_size(&entry.path())?;
        entries.push(Entry {
            name: entry.file_name().into(),
            kind,
            bytes,
            empty_tree,
        });
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

pub(super) fn move_aside(old: &Path, new: &Path) -> io::Result<String> {
    let entries = inventory(old)?;
    let contents = entries
        .iter()
        .map(|entry| {
            format!(
                "{} ({} bytes, {:?})",
                entry.name.display(),
                entry.bytes,
                entry.kind
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let refusal = || {
        io::Error::other(format!(
        "Cannot relocate data: both {} and {} exist; legacy contents: [{}]; destination must be a complete data root and legacy content must be recreatable",
        old.display(), new.display(), contents,
    ))
    };
    if !recreatable(&entries) || validate_schema(new, false).is_err() {
        return Err(refusal());
    }
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let aside = old.with_file_name(format!(
        "{}-aside-{timestamp}",
        old.file_name().unwrap().to_string_lossy()
    ));
    if path_exists(&aside)? {
        return Err(io::Error::other(format!(
            "Relocation aside already exists: {}",
            aside.display()
        )));
    }
    rename_legacy_root(old, &aside, &SystemLegacyRootHolderProbe)?;
    let notice = format!(
        "Moved recreated legacy content from {} to {}: [{}]",
        old.display(),
        aside.display(),
        contents
    );
    log::info!("[upgrade] {notice}");
    Ok(notice)
}

#[cfg(test)]
mod tests;
