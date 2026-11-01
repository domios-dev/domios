use std::{
    io::{self, Write},
    path::{Path, PathBuf},
};

pub struct PidFile {
    temporary: tempfile::NamedTempFile,
    destination: PathBuf,
}

impl PidFile {
    pub fn prepare(destination: &Path) -> io::Result<Self> {
        let exists = match destination.symlink_metadata() {
            Ok(_) => true,
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => return Err(error),
        };
        if !destination.is_absolute() || exists {
            return Err(io::Error::other("PID file must be a new absolute path"));
        }
        let parent = destination
            .parent()
            .ok_or_else(|| io::Error::other("PID file requires a parent directory"))?;
        Ok(Self {
            temporary: tempfile::NamedTempFile::new_in(parent)?,
            destination: destination.into(),
        })
    }

    pub fn publish(mut self, pid: u32) -> io::Result<()> {
        if pid == 0 {
            return Err(io::Error::other("Server PID must be nonzero"));
        }
        writeln!(self.temporary, "{pid}")?;
        self.temporary.as_file().sync_all()?;
        self.temporary
            .persist_noclobber(self.destination)
            .map_err(|error| error.error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
