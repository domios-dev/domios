use super::*;

pub(super) fn seal_files(
    envelope: &Envelope,
    paths: Vec<String>,
    pad: Option<crate::service::agent_pad::AgentPad>,
) -> codeterm_neighbors::Result<Vec<(Manifest, Vec<u8>)>> {
    if paths.is_empty() || paths.len() > files::MAX_ATTACHMENTS {
        return Err(Error::InvalidRequest);
    }
    use std::io::Read;
    let mut sealed = vec![];
    for path in paths {
        let mut file = match &pad {
            Some(pad) => pad.open_file(&path).map_err(|_| Error::InvalidRequest)?,
            None => {
                if !std::path::Path::new(&path).is_absolute() {
                    return Err(Error::InvalidRequest);
                }
                if !std::fs::metadata(&path)
                    .map_err(|_| Error::InvalidRequest)?
                    .is_file()
                {
                    return Err(Error::InvalidRequest);
                }
                std::fs::File::open(&path).map_err(|_| Error::InvalidRequest)?
            }
        };
        if !file
            .metadata()
            .map_err(|_| Error::InvalidRequest)?
            .is_file()
        {
            return Err(Error::InvalidRequest);
        }
        let mut bytes = Zeroizing::new(Vec::new());
        file.by_ref()
            .take((files::MAX_FILE_BYTES - files::FILE_OVERHEAD + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::InvalidRequest)?;
        if bytes.len() > files::MAX_FILE_BYTES - files::FILE_OVERHEAD {
            return Err(Error::File(files::refusal::Failure::too_large()));
        }
        let name = std::path::Path::new(&path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("attachment");
        sealed.push(files::seal(
            &binding(envelope),
            &uuid::Uuid::new_v4().simple().to_string(),
            name,
            "application/octet-stream",
            &bytes,
            &mut rand::rngs::OsRng,
        )?);
    }
    Ok::<_, Error>(sealed)
}

#[cfg(test)]
mod tests;
