use super::*;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::AsRawHandle;
use std::path::PathBuf;
use windows_sys::Win32::Storage::FileSystem::{
    GetFinalPathNameByHandleW, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_NAME_NORMALIZED, FILE_SHARE_READ,
};

pub fn open_file(directory: &Path, relative: &str) -> Result<File, &'static str> {
    let parts = Path::new(relative)
        .components()
        .map(|component| {
            let Component::Normal(name) = component else {
                return Err("contract_violation");
            };
            Ok(name)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if parts.is_empty() {
        return Err("contract_violation");
    }
    let mut path = directory.to_path_buf();
    let root_handle = open_directory(&path)?;
    if !root_handle
        .metadata()
        .is_ok_and(|metadata| metadata.is_dir())
    {
        return Err("contract_violation");
    }
    let root = final_path(&root_handle)?;
    let mut handles = vec![root_handle];
    for (index, part) in parts.iter().enumerate() {
        path.push(part);
        let metadata = std::fs::symlink_metadata(&path).map_err(|_| "contract_violation")?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err("contract_violation");
        }
        let handle = open(&path)?;
        let metadata = handle.metadata().map_err(|_| "contract_violation")?;
        if !final_path(&handle)?.starts_with(&root)
            || if index + 1 == parts.len() {
                !metadata.is_file()
            } else {
                !metadata.is_dir()
            }
        {
            return Err("contract_violation");
        }
        handles.push(handle);
    }
    handles.pop().ok_or("contract_violation")
}

pub fn open_directory(path: &Path) -> Result<File, &'static str> {
    let file = open(path)?;
    if !file.metadata().is_ok_and(|value| value.is_dir()) {
        return Err("contract_violation");
    }
    Ok(file)
}

fn open(path: &Path) -> Result<File, &'static str> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .map_err(|_| "contract_violation")?;
    if file
        .metadata()
        .map_err(|_| "contract_violation")?
        .file_attributes()
        & FILE_ATTRIBUTE_REPARSE_POINT
        != 0
    {
        return Err("contract_violation");
    }
    Ok(file)
}

fn final_path(file: &File) -> Result<PathBuf, &'static str> {
    let handle = file.as_raw_handle() as _;
    let required =
        unsafe { GetFinalPathNameByHandleW(handle, std::ptr::null_mut(), 0, FILE_NAME_NORMALIZED) };
    if required == 0 || required > 32768 {
        return Err("contract_violation");
    }
    let mut name = vec![0; required as usize + 1];
    let written = unsafe {
        GetFinalPathNameByHandleW(
            handle,
            name.as_mut_ptr(),
            name.len() as u32,
            FILE_NAME_NORMALIZED,
        )
    };
    if written == 0 || written as usize >= name.len() {
        return Err("contract_violation");
    }
    Ok(PathBuf::from(OsString::from_wide(
        &name[..written as usize],
    )))
}
