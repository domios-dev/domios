use std::{fs::File, io, path::Path};

#[cfg(windows)]
mod windows;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct FileIdentity {
    volume: u64,
    file: [u8; 16],
}

pub fn file_identity(path: &Path) -> io::Result<Option<FileIdentity>> {
    match std::fs::metadata(path) {
        Ok(metadata) if !metadata.is_file() => return Ok(None),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    }
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    identity(&file).map(Some)
}

pub fn identity(file: &File) -> io::Result<FileIdentity> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = file.metadata()?;
        let mut id = [0; 16];
        id[..8].copy_from_slice(&metadata.ino().to_le_bytes());
        Ok(FileIdentity {
            volume: metadata.dev(),
            file: id,
        })
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FileIdInfo, GetFileInformationByHandleEx, FILE_ID_INFO,
        };
        let mut info: FILE_ID_INFO = unsafe { std::mem::zeroed() };
        if unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileIdInfo,
                (&mut info as *mut FILE_ID_INFO).cast(),
                std::mem::size_of_val(&info) as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(FileIdentity {
            volume: info.VolumeSerialNumber,
            file: info.FileId.Identifier,
        })
    }
}

pub fn directory_link(target: &Path, link: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        windows::junction(target, link)
    }
}

pub fn file_link(target: &Path, link: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(target, link).or_else(|symlink| {
            std::fs::hard_link(target, link).map_err(|hardlink| io::Error::new(io::ErrorKind::Unsupported,
                format!("Shared file linking unsupported on this machine: symbolic link: {symlink}; hard link: {hardlink}")))
        })
    }
}
