use codeterm_types::server_bootstrap::ServerComponentManifest;
use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

pub(crate) fn load(resources: &Path) -> io::Result<Option<ServerComponentManifest>> {
    let path = resources.join("server-component.json");
    match path.symlink_metadata() {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Server component inventory must be a regular file",
            ));
        }
        Ok(_) => {}
    }
    let mut bytes = Vec::new();
    File::open(path)?.take(8193).read_to_end(&mut bytes)?;
    parse(&bytes).map(Some)
}

fn parse(bytes: &[u8]) -> io::Result<ServerComponentManifest> {
    if bytes.is_empty() || bytes.len() > 8192 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Server component inventory exceeds bound",
        ));
    }
    serde_json::from_slice(bytes).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid server component inventory",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> serde_json::Value {
        serde_json::json!({
            "schema": 2, "server_build": "server-build",
            "server_executable": "domios-server/domios-server",
            "server_sha256": "a".repeat(64), "client_identity": {"kind":"sha256", "sha256":"b".repeat(64)},
            "api_version": 3, "bootstrap_protocol": codeterm_types::server_bootstrap::BOOTSTRAP_PROTOCOL,
        })
    }

    #[test]
    fn process_split_contract_missing_native_inventory_preserves_headless_startup() {
        let resources = tempfile::tempdir().unwrap();
        assert!(load(resources.path()).unwrap().is_none());
        assert_eq!(std::fs::read_dir(resources.path()).unwrap().count(), 0);
    }

    #[test]
    fn process_split_contract_present_native_inventory_never_falls_back() {
        let resources = tempfile::tempdir().unwrap();
        let path = resources.path().join("server-component.json");
        for bytes in [Vec::new(), b"{".to_vec(), vec![b' '; 8193]] {
            std::fs::write(&path, &bytes).unwrap();
            assert!(load(resources.path()).is_err());
        }
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(load(resources.path()).is_err());
    }

    #[test]
    fn process_split_contract_native_inventory_requires_exact_schema_fields() {
        let value = manifest();
        let parsed = parse(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), value);
        let mut unknown = value.clone();
        unknown["fallback"] = true.into();
        assert!(parse(&serde_json::to_vec(&unknown).unwrap()).is_err());
        let mut missing = value;
        missing.as_object_mut().unwrap().remove("client_identity");
        assert!(parse(&serde_json::to_vec(&missing).unwrap()).is_err());
    }
}
