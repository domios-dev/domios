use sha2::{Digest, Sha256};

pub const VARIABLES: [&str; 4] = [
    "DOMIOS_FIREBASE_API_KEY",
    "DOMIOS_FIREBASE_AUTH_DOMAIN",
    "DOMIOS_FIREBASE_PROJECT_ID",
    "DOMIOS_FIREBASE_APP_ID",
];

pub fn validate(release: bool, values: &[Option<String>; 4]) -> Result<(), String> {
    if release {
        let missing: Vec<_> = [0, 2, 3]
            .into_iter()
            .filter(|index| {
                values[*index]
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty())
            })
            .map(|index| VARIABLES[index])
            .collect();
        if !missing.is_empty() {
            return Err(format!(
                "Firebase release config missing: {}. Set the GitHub repository build configuration before building.",
                missing.join(", ")
            ));
        }
    }
    Ok(())
}

pub fn resolve_auth_domain(values: &mut [Option<String>; 4]) {
    if values[1]
        .as_deref()
        .is_none_or(|value| value.trim().is_empty())
    {
        values[1] = values[2]
            .as_deref()
            .filter(|project| !project.trim().is_empty())
            .map(|project| format!("{project}.firebaseapp.com"));
    }
}

pub fn fingerprint(values: &[Option<String>; 4]) -> String {
    let mut digest = Sha256::new();
    for value in values {
        let bytes = value.as_deref().unwrap_or_default().as_bytes();
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }
    format!("{:x}", digest.finalize())[..8].to_owned()
}

#[cfg(test)]
mod tests;
