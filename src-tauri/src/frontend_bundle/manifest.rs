use minisign_verify::{PublicKey, Signature};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub version: String,
    pub min_api_version: u32,
    pub max_api_version: u32,
    pub sha256: String,
    pub url: String,
    pub release_id: String,
}

#[derive(Clone, Debug)]
pub struct VerifiedManifest(Manifest);

impl VerifiedManifest {
    pub fn manifest(&self) -> &Manifest {
        &self.0
    }
}

pub fn safe_component(value: &str) -> bool {
    value
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_alphanumeric())
        && super::store::safe_asset_path(value)
        && value.len() <= 128
        && value != "."
        && value != ".."
        && !value.ends_with('.')
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
}

impl Manifest {
    pub fn compatible(&self, api_version: u32) -> bool {
        self.min_api_version > 0
            && self.min_api_version <= api_version
            && api_version <= self.max_api_version
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        let fields = BTreeMap::from([
            ("version", serde_json::json!(self.version)),
            ("minApiVersion", serde_json::json!(self.min_api_version)),
            ("maxApiVersion", serde_json::json!(self.max_api_version)),
            ("sha256", serde_json::json!(self.sha256)),
            ("url", serde_json::json!(self.url)),
            ("releaseId", serde_json::json!(self.release_id)),
        ]);
        serde_json::to_vec(&fields).map_err(|e| e.to_string())
    }
}

pub fn verify(
    metadata: &[u8],
    signature: &str,
    archive: &[u8],
    trusted_public_key: &str,
    api_version: u32,
    allow_loopback: bool,
) -> Result<VerifiedManifest, String> {
    let manifest = verify_metadata(
        metadata,
        signature,
        trusted_public_key,
        api_version,
        allow_loopback,
    )?;
    let digest = format!("{:x}", Sha256::digest(archive));
    if digest != manifest.sha256 {
        return Err("Archive SHA256 mismatch".into());
    }
    Ok(VerifiedManifest(manifest))
}

pub fn verify_metadata(
    metadata: &[u8],
    signature: &str,
    trusted_public_key: &str,
    api_version: u32,
    allow_loopback: bool,
) -> Result<Manifest, String> {
    let manifest: Manifest = serde_json::from_slice(metadata).map_err(|e| e.to_string())?;
    if !manifest.compatible(api_version) || !safe_component(&manifest.release_id) {
        return Err("Invalid release identity or incompatible API range".into());
    }
    semver::Version::parse(&manifest.version).map_err(|e| e.to_string())?;
    if manifest.sha256.len() != 64
        || !manifest
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("Invalid SHA256".into());
    }
    validate_url(&manifest.url, allow_loopback)?;
    let key = PublicKey::from_base64(trusted_public_key).map_err(|e| e.to_string())?;
    let signature = Signature::decode(signature).map_err(|e| e.to_string())?;
    key.verify(&manifest.canonical_bytes()?, &signature, false)
        .map_err(|e| e.to_string())?;
    Ok(manifest)
}

pub fn validate_url(value: &str, allow_loopback: bool) -> Result<(), String> {
    let url = url::Url::parse(value).map_err(|e| e.to_string())?;
    let loopback = match url.host() {
        Some(url::Host::Domain(host)) => host == "localhost",
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    };
    if url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.query().is_some()
        || !(url.scheme() == "https" || (allow_loopback && loopback && url.scheme() == "http"))
    {
        return Err("Frontend URL requires HTTPS or explicit DEV loopback".into());
    }
    Ok(())
}
