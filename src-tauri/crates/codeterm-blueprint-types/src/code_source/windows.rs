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

pub fn read(directory: &Path, relative: &str) -> Result<String, &'static str> {
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
    let mut path = directory.canonicalize().map_err(|_| "contract_violation")?;
    let root_handle = open(&path)?;
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
    let file = handles.pop().ok_or("contract_violation")?;
    let mut text = String::new();
    file.take(16385)
        .read_to_string(&mut text)
        .map_err(|_| "contract_violation")?;
    if text.len() > 16384 {
        return Err("budget_exhausted");
    }
    Ok(text)
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
