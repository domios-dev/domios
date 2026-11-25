use codeterm_types::{
    config::schema::wallpaper_asset_format,
    svg::{sanitize_svg, sanitize_wallpaper_upload, SvgRefusal, SvgUse, MAX_CHAT_TILE_BYTES},
};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

mod raster;

pub const UPLOAD_LIMIT: usize = 2 * 1024 * 1024;
pub const ASSET_LIMIT: usize = 16 * 1024 * 1024;
const MAX_ASSETS: usize = 12;
const DIRECTORY: &str = "chat-wallpapers";

pub fn is_wallpaper_asset_path(root: &Path, path: &Path) -> bool {
    path.starts_with(root.join(DIRECTORY))
}

#[derive(Debug)]
pub enum AssetError {
    TooLarge,
    Unsupported,
    Invalid,
    Svg(SvgRefusal),
    Full,
    Missing,
    Storage(io::Error),
}

impl From<io::Error> for AssetError {
    fn from(error: io::Error) -> Self {
        Self::Storage(error)
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Png,
    Svg,
}

impl Kind {
    fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Svg => "svg",
        }
    }

    fn media_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Svg => "image/svg+xml",
        }
    }
}

fn normalize(media_type: &str, input: &[u8]) -> Result<(Kind, Vec<u8>), AssetError> {
    if input.len() > UPLOAD_LIMIT
        || (media_type == "image/svg+xml" && input.len() > MAX_CHAT_TILE_BYTES)
    {
        return Err(AssetError::TooLarge);
    }
    match media_type {
        "image/svg+xml" => sanitize_wallpaper_upload(input)
            .map(|svg| (Kind::Svg, svg.into_bytes()))
            .map_err(AssetError::Svg),
        "image/png" | "image/jpeg" | "image/webp" => {
            raster::normalize(media_type, input).map(|png| (Kind::Png, png))
        }
        _ => Err(AssetError::Unsupported),
    }
}

fn asset_name(kind: Kind, bytes: &[u8]) -> String {
    format!("{:x}.{}", Sha256::digest(bytes), kind.extension())
}

fn asset_kind(name: &str) -> Result<Kind, AssetError> {
    match wallpaper_asset_format(name) {
        Some("png") => Ok(Kind::Png),
        Some("svg") => Ok(Kind::Svg),
        _ => Err(AssetError::Missing),
    }
}

pub struct WallpaperStore {
    data_dir: PathBuf,
    writer: parking_lot::Mutex<()>,
}

impl WallpaperStore {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            writer: parking_lot::Mutex::new(()),
        }
    }

    pub fn save(&self, media_type: &str, input: &[u8]) -> Result<String, AssetError> {
        let (kind, bytes) = normalize(media_type, input)?;
        let name = asset_name(kind, &bytes);
        self.persist(&name, &bytes)?;
        Ok(name)
    }

    pub fn copy(&self, name: &str, input: &[u8]) -> Result<(), AssetError> {
        let kind = asset_kind(name)?;
        if input.len() > ASSET_LIMIT || asset_name(kind, input) != name {
            return Err(AssetError::Invalid);
        }
        let clean = match kind {
            Kind::Svg => sanitize_svg(input, SvgUse::ChatTile)
                .ok_or(AssetError::Invalid)?
                .into_bytes(),
            Kind::Png => raster::normalize(kind.media_type(), input)?,
        };
        if clean != input {
            return Err(AssetError::Invalid);
        }
        self.persist(name, &clean)
    }

    fn persist(&self, name: &str, bytes: &[u8]) -> Result<(), AssetError> {
        let _writer = self.writer.lock();
        fs::create_dir_all(&self.data_dir)?;
        let directory = self.directory(true)?;
        let path = directory.join(name);
        if path.exists() {
            self.read(name)?;
            return Ok(());
        }
        let mut assets = 0;
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if entry
                .file_name()
                .to_str()
                .is_some_and(|name| asset_kind(name).is_ok())
            {
                assets += 1;
                if assets >= MAX_ASSETS {
                    return Err(AssetError::Full);
                }
            }
        }
        codeterm_util::private_file::write_atomic(&path, bytes)?;
        Ok(())
    }

    pub fn read(&self, name: &str) -> Result<(&'static str, Vec<u8>), AssetError> {
        let kind = asset_kind(name)?;
        let path = self.directory(false)?.join(name);
        let metadata = fs::symlink_metadata(&path).map_err(missing)?;
        if !metadata.is_file() || redirected(&metadata) || path.canonicalize()? != path {
            return Err(AssetError::Missing);
        }
        let file = open_asset(&path).map_err(missing)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || redirected(&metadata) || metadata.len() > ASSET_LIMIT as u64 {
            return Err(AssetError::Missing);
        }
        let mut bytes = Vec::new();
        file.take((ASSET_LIMIT + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > ASSET_LIMIT || asset_name(kind, &bytes) != name {
            return Err(AssetError::Missing);
        }
        let valid = match kind {
            Kind::Svg => {
                sanitize_svg(&bytes, SvgUse::ChatTile).is_some_and(|svg| svg.as_bytes() == bytes)
            }
            Kind::Png => raster::is_canonical_png(&bytes),
        };
        if !valid {
            return Err(AssetError::Missing);
        }
        Ok((kind.media_type(), bytes))
    }

    pub fn remove(&self, name: &str) -> Result<(), AssetError> {
        asset_kind(name)?;
        let _writer = self.writer.lock();
        let path = self.directory(false)?.join(name);
        let metadata = fs::symlink_metadata(&path).map_err(missing)?;
        if !metadata.is_file() || redirected(&metadata) {
            return Err(AssetError::Missing);
        }
        fs::remove_file(path).map_err(missing)
    }

    fn directory(&self, create: bool) -> Result<PathBuf, AssetError> {
        let directory = self
            .data_dir
            .canonicalize()
            .map_err(missing)?
            .join(DIRECTORY);
        if create {
            match fs::create_dir(&directory) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
        let metadata = fs::symlink_metadata(&directory).map_err(missing)?;
        if !metadata.is_dir() || redirected(&metadata) || directory.canonicalize()? != directory {
            return Err(AssetError::Missing);
        }
        Ok(directory)
    }
}

fn redirected(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    metadata.file_type().is_symlink()
}

fn open_asset(path: &Path) -> io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options.open(path)
}

fn missing(error: io::Error) -> AssetError {
    if error.kind() == io::ErrorKind::NotFound {
        AssetError::Missing
    } else {
        error.into()
    }
}

#[cfg(test)]
mod tests;
