use super::*;
use std::{
    ffi::{c_void, OsStr, OsString},
    io::Write,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::{MetadataExt, OpenOptionsExt},
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Component, PathBuf},
};
use windows_sys::Win32::{
    Foundation::{GENERIC_READ, GENERIC_WRITE, HANDLE},
    Storage::FileSystem::{
        GetFileInformationByHandle, GetFinalPathNameByHandleW, BY_HANDLE_FILE_INFORMATION,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    },
};

#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *mut u16,
}
#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root: HANDLE,
    name: *mut UnicodeString,
    attributes: u32,
    security: *mut c_void,
    quality: *mut c_void,
}
#[repr(C)]
struct IoStatus {
    status: usize,
    information: usize,
}
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtCreateFile(
        handle: *mut HANDLE,
        access: u32,
        attributes: *mut ObjectAttributes,
        status: *mut IoStatus,
        allocation: *const i64,
        file_attributes: u32,
        sharing: u32,
        disposition: u32,
        options: u32,
        ea: *const c_void,
        ea_length: u32,
    ) -> i32;
    fn NtSetInformationFile(
        handle: HANDLE,
        status: *mut IoStatus,
        information: *const c_void,
        length: u32,
        class: u32,
    ) -> i32;
    fn RtlNtStatusToDosError(status: i32) -> u32;
}

fn error() -> String {
    std::io::Error::last_os_error().to_string()
}

pub(in super::super) fn relative(root: &Path, raw: &str) -> Result<PathBuf, String> {
    let relative = relative_directory(root, raw)?;
    if relative.as_os_str().is_empty() {
        return Err("Expected a workspace file".into());
    }
    Ok(relative)
}

pub(in super::super) fn relative_directory(root: &Path, raw: &str) -> Result<PathBuf, String> {
    if raw.is_empty() || raw.len() > MAX_PATH || raw.chars().any(char::is_control) {
        return Err("Invalid bridge path".into());
    }
    let path = disk_path(Path::new(raw))?;
    let root = disk_path(root)?;
    Ok(path
        .strip_prefix(root)
        .map_err(|_| "Path outside workspace")?
        .to_path_buf())
}

fn valid_name(name: &OsStr) -> Result<(), String> {
    let name = name.to_str().ok_or("Invalid UTF-8 path")?;
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if name.is_empty()
        || name.chars().any(char::is_control)
        || name.contains([':', '/', '\\'])
        || name.ends_with(['.', ' '])
        || ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&stem.as_str())
        || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|suffix| {
                matches!(
                    suffix,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        })
    {
        return Err("Invalid Windows file component".into());
    }
    Ok(())
}

fn disk_path(path: &Path) -> Result<PathBuf, String> {
    use std::path::Prefix;
    let raw = path.to_str().ok_or("Invalid UTF-8 path")?;
    let mut parts = path.components();
    let Some(Component::Prefix(prefix)) = parts.next() else {
        return Err("Expected absolute drive path".into());
    };
    let drive = match prefix.kind() {
        Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => drive,
        _ => return Err("Network and device paths denied".into()),
    };
    if parts.next() != Some(Component::RootDir)
        || raw
            .split(['/', '\\'])
            .any(|part| part == "." || part == "..")
    {
        return Err("Drive-relative and traversal paths denied".into());
    }
    let mut result = PathBuf::from(format!("{}:\\", drive as char));
    for part in parts {
        let Component::Normal(name) = part else {
            return Err("Invalid path component".into());
        };
        valid_name(name)?;
        result.push(name);
    }
    Ok(result)
}

fn validate(file: &File, directory: bool) -> Result<(), String> {
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err("Reparse traversal denied".into());
    }
    if if directory {
        !metadata.is_dir()
    } else {
        !metadata.is_file()
    } {
        return Err("Unexpected file type".into());
    }
    Ok(())
}

fn open_at(parent: &File, name: &OsStr, directory: bool, create: bool) -> Result<File, String> {
    valid_name(name)?;
    let mut name: Vec<_> = name.encode_wide().collect();
    let length = u16::try_from(name.len() * 2).map_err(|_| "Path component too long")?;
    let mut name = UnicodeString {
        length,
        maximum_length: length,
        buffer: name.as_mut_ptr(),
    };
    let mut attributes = ObjectAttributes {
        length: std::mem::size_of::<ObjectAttributes>() as u32,
        root: parent.as_raw_handle(),
        name: &mut name,
        attributes: 0x1040, // OBJ_DONT_REPARSE rejects reparse processing in the object manager too.
        security: std::ptr::null_mut(),
        quality: std::ptr::null_mut(),
    };
    let mut status = IoStatus {
        status: 0,
        information: 0,
    };
    let mut handle = std::ptr::null_mut();
    let result = unsafe {
        NtCreateFile(
            &mut handle,
            GENERIC_READ
                | if create {
                    GENERIC_WRITE | 0x0001_0000
                } else {
                    0
                },
            &mut attributes,
            &mut status,
            std::ptr::null(),
            0,
            FILE_SHARE_READ
                | if directory || create {
                    0
                } else {
                    FILE_SHARE_WRITE | FILE_SHARE_DELETE
                },
            if create { 2 } else { 1 },
            0x0020_0020 | if directory { 1 } else { 0x40 },
            std::ptr::null(),
            0,
        )
    };
    if result < 0 {
        return Err(std::io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(result) } as i32
        )
        .to_string());
    }
    let file = unsafe { File::from_raw_handle(handle) };
    validate(&file, directory)?;
    Ok(file)
}

pub(in super::super) fn pinned(path: &Path) -> Result<Vec<File>, String> {
    let path = disk_path(path)?;
    let mut parts = path.components();
    let prefix = parts.next().ok_or("Missing drive")?;
    let mut drive = PathBuf::from(prefix.as_os_str());
    drive.push("\\");
    let _ = parts.next();
    let root = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(drive)
        .map_err(|error| error.to_string())?;
    validate(&root, true)?;
    let mut handles = vec![root];
    for part in parts {
        let Component::Normal(name) = part else {
            return Err("Invalid path component".into());
        };
        let parent = handles.last().ok_or("Missing parent")?;
        handles.push(open_at(parent, name, true, false)?);
    }
    Ok(handles)
}

pub(in super::super) fn open_root(path: &Path) -> Result<File, String> {
    pinned(path)?.pop().ok_or("Missing workspace root".into())
}

pub(in super::super) fn same(a: &File, b: &File) -> Result<(), String> {
    fn identity(file: &File) -> Result<(u32, u32, u32), String> {
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(error());
        }
        Ok((
            info.dwVolumeSerialNumber,
            info.nFileIndexHigh,
            info.nFileIndexLow,
        ))
    }
    if identity(a)? != identity(b)? {
        return Err("Workspace directory replaced".into());
    }
    Ok(())
}

pub(in super::super) fn final_path(file: &File) -> Result<PathBuf, String> {
    let mut buffer = vec![0; 32768];
    let count = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            0,
        )
    };
    if count == 0 || count as usize >= buffer.len() {
        return Err(error());
    }
    Ok(PathBuf::from(OsString::from_wide(
        &buffer[..count as usize],
    )))
}

fn directory_chain(root: &File, relative: &Path) -> Result<Vec<File>, String> {
    let mut handles = vec![root.try_clone().map_err(|error| error.to_string())?];
    for part in relative.components() {
        let Component::Normal(name) = part else {
            return Err("Invalid directory component".into());
        };
        handles.push(open_at(
            handles.last().ok_or("Missing parent")?,
            name,
            true,
            false,
        )?);
    }
    Ok(handles)
}

pub(in super::super) fn directory(root: &File, relative: &Path) -> Result<File, String> {
    directory_chain(root, relative)?
        .pop()
        .ok_or("Missing directory".into())
}

pub(in super::super) fn read(root: &File, relative: &Path) -> Result<File, String> {
    let handles = directory_chain(root, relative.parent().ok_or("Missing parent")?)?;
    open_at(
        handles.last().ok_or("Missing parent")?,
        relative.file_name().ok_or("Missing filename")?,
        false,
        false,
    )
}

pub(in super::super) fn root_matches(root: &Path, held: &File) -> Result<(), String> {
    same(&open_root(root)?, held)
}

fn rename(file: &File, parent: &File, name: &OsStr) -> Result<(), String> {
    #[repr(C)]
    struct RenameInformation {
        replace: u8,
        root: HANDLE,
        name_length: u32,
        name: [u16; 1],
    }
    let name: Vec<_> = name.encode_wide().collect();
    let offset = std::mem::offset_of!(RenameInformation, name);
    let length = (offset + name.len() * 2).max(std::mem::size_of::<RenameInformation>());
    let mut storage = vec![0usize; length.div_ceil(std::mem::size_of::<usize>())];
    let information = storage.as_mut_ptr().cast::<RenameInformation>();
    unsafe {
        (*information).replace = 1;
        (*information).root = parent.as_raw_handle();
        (*information).name_length = (name.len() * 2) as u32;
        std::ptr::copy_nonoverlapping(
            name.as_ptr(),
            information.cast::<u8>().add(offset).cast::<u16>(),
            name.len(),
        );
    }
    let mut status = IoStatus {
        status: 0,
        information: 0,
    };
    let result = unsafe {
        NtSetInformationFile(
            file.as_raw_handle(),
            &mut status,
            information.cast(),
            length as u32,
            10,
        )
    };
    if result < 0 {
        return Err(std::io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(result) } as i32
        )
        .to_string());
    }
    file.sync_all().map_err(|error| error.to_string())
}

pub(in super::super) fn write(
    root: &Path,
    root_handle: &File,
    relative: &Path,
    content: &str,
    commit: impl FnOnce(&mut dyn FnMut() -> Result<(), String>) -> Result<(), String>,
) -> Result<(), String> {
    if content.len() > MAX_BYTES {
        return Err("Write exceeds 1 MiB".into());
    }
    let mut handles = pinned(root)?;
    same(handles.last().ok_or("Missing root")?, root_handle)?;
    let parent = relative.parent().ok_or("Missing parent")?;
    for part in parent.components() {
        let Component::Normal(name) = part else {
            return Err("Invalid directory component".into());
        };
        handles.push(open_at(
            handles.last().ok_or("Missing parent")?,
            name,
            true,
            false,
        )?);
    }
    let held = handles.last().ok_or("Missing parent")?;
    let name = relative.file_name().ok_or("Missing filename")?;
    valid_name(name)?;
    check(root, relative, true)?;
    let temporary = OsString::from(format!(".domios-bridge-{}", uuid::Uuid::new_v4()));
    let parent_path = final_path(held)?;
    let mut file = open_at(held, &temporary, false, true)?;
    let temporary_path = parent_path.join(&temporary);
    let result = (|| {
        file.write_all(content.as_bytes())
            .map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        let mut apply = || {
            root_matches(root, root_handle)?;
            same(&directory(root_handle, parent)?, held)?;
            check(root, relative, true)?;
            rename(&file, held, name)
        };
        commit(&mut apply)
    })();
    drop(file);
    if result.is_err() {
        let _ = std::fs::remove_file(temporary_path);
    }
    result
}

pub(in super::super) fn check(
    root: &Path,
    relative: &Path,
    missing_leaf: bool,
) -> Result<(), String> {
    let root_chain = pinned(root)?;
    let held = root_chain.last().ok_or("Missing root")?;
    let parents = directory_chain(held, relative.parent().ok_or("Missing parent")?)?;
    let parent = parents.last().ok_or("Missing parent")?;
    let name = relative.file_name().ok_or("Missing filename")?;
    valid_name(name)?;
    match std::fs::symlink_metadata(final_path(parent)?.join(name)) {
        Ok(metadata) if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 => {
            Err("Reparse traversal denied".into())
        }
        Ok(metadata) if !metadata.is_file() => Err("Non-regular file denied".into()),
        Ok(_) => Ok(()),
        Err(error) if missing_leaf && error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}
