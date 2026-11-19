use std::fs::{File, OpenOptions};
use std::io;
use std::mem::{offset_of, size_of};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Component, Path, PathBuf};
use std::ptr::{copy_nonoverlapping, null_mut};
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Storage::FileSystem::{
    FileRenameInfo, GetFileInformationByHandle, SetFileInformationByHandle,
    BY_HANDLE_FILE_INFORMATION, DELETE, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

pub(super) fn move_aside<G>(
    home: &Path,
    leaf: &str,
    timestamp: u64,
    validate_binding: impl FnOnce() -> Result<G, String>,
) -> Result<PathBuf, String> {
    let home = codeterm_util::safe_tree::extended_path(home).map_err(|error| error.to_string())?;
    let mut ancestors = Vec::new();
    let mut current = PathBuf::new();
    for component in home.components() {
        current.push(component);
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        let directory = open(&current, false)?;
        if !directory
            .metadata()
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            return Err("The credential home is not a directory".into());
        }
        ancestors.push(directory);
    }
    let source = open(&home.join(leaf), true)?;
    if !source
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("The saved sign-in is not a regular file".into());
    }
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(source.as_raw_handle(), &mut information) } == 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    if information.nNumberOfLinks != 1 {
        return Err("Recovery refuses a hard-linked credential file".into());
    }
    let backup = home.join(format!(
        "{leaf}.backup-{timestamp}-{}",
        uuid::Uuid::new_v4()
    ));
    let _binding = validate_binding()?;
    rename(&source, &backup)?;
    drop(ancestors);
    Ok(backup)
}

fn open(path: &Path, credential: bool) -> Result<File, String> {
    let file = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES | if credential { DELETE } else { 0 })
        .share_mode(FILE_SHARE_READ | if credential { 0 } else { FILE_SHARE_WRITE })
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if codeterm_util::safe_tree::is_link(&metadata) {
        return Err("Recovery refuses symlinks and reparse points".into());
    }
    Ok(file)
}

fn rename(file: &File, backup: &Path) -> Result<(), String> {
    #[repr(C)]
    struct RenameInformation {
        replace: u32,
        root: HANDLE,
        name_length: u32,
        name: [u16; 1],
    }
    let name: Vec<u16> = backup.as_os_str().encode_wide().collect();
    let offset = offset_of!(RenameInformation, name);
    let length = (offset + name.len() * 2).max(size_of::<RenameInformation>());
    let mut storage = vec![0usize; length.div_ceil(size_of::<usize>())];
    let information = storage.as_mut_ptr().cast::<RenameInformation>();
    unsafe {
        (*information).replace = 0;
        (*information).root = null_mut();
        (*information).name_length = (name.len() * 2) as u32;
        copy_nonoverlapping(
            name.as_ptr(),
            information.cast::<u8>().add(offset).cast::<u16>(),
            name.len(),
        );
        if SetFileInformationByHandle(
            file.as_raw_handle(),
            FileRenameInfo,
            information.cast(),
            length as u32,
        ) == 0
        {
            return Err(io::Error::last_os_error().to_string());
        }
    }
    Ok(())
}
