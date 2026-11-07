use std::fs;
use std::path::Path;

pub fn sweep_launch_files(data_dir: &Path) -> Result<(), String> {
    let root = data_dir.join("runtime").join("agent-pads");
    match fs::symlink_metadata(&root) {
        Ok(metadata) if ordinary_directory(&metadata) => {}
        Ok(_) => return Err("launch handoff root is not an ordinary directory".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("inspect launch handoff root: {error}")),
    }
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("read launch handoff root: {error}")),
    };
    for pad in entries {
        let pad = pad.map_err(|error| format!("read launch handoff pad: {error}"))?;
        let metadata = fs::symlink_metadata(pad.path()).map_err(|error| error.to_string())?;
        if !ordinary_directory(&metadata) {
            continue;
        }
        for file in fs::read_dir(pad.path()).map_err(|error| error.to_string())? {
            let file = file.map_err(|error| error.to_string())?;
            if codeterm_util::tab_launch::is_handoff_file(&file.file_name().to_string_lossy()) {
                fs::remove_file(file.path())
                    .map_err(|error| format!("remove stale launch handoff: {error}"))?;
            }
        }
    }
    Ok(())
}

fn ordinary_directory(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
        {
            return false;
        }
    }
    metadata.is_dir() && !metadata.file_type().is_symlink()
}

#[cfg(test)]
mod tests;
