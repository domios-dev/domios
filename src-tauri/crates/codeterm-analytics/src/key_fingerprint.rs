use sha2::{Digest, Sha256};

pub(crate) fn fingerprint(key: Option<&str>) -> String {
    match key.filter(|value| !value.trim().is_empty()) {
        Some(value) => format!("{:x}", Sha256::digest(value.as_bytes()))[..8].to_owned(),
        None => "none".to_owned(),
    }
}

#[cfg(test)]
mod tests;
