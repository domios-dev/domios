//! Identity management — signing, authentication, keypair operations.

use super::crown;
use super::types::MeshIdentity;

pub fn instance_mesh_name_for_dev(configured: &str, instance_suffix: &str) -> String {
    let base = configured
        .rfind(" [dev")
        .filter(|_| configured.ends_with(']'))
        .map(|index| &configured[..index])
        .unwrap_or(configured);
    let marker = if instance_suffix.is_empty() {
        " [dev]".to_string()
    } else {
        format!(" [dev{}]", instance_suffix)
    };
    format!("{base}{marker}")
}

/// Manages this instance's identity and request signing/verification.
pub struct IdentityManager {
    pub identity: MeshIdentity,
    pub instance_name: String,
}

impl IdentityManager {
    pub fn new(identity: MeshIdentity, instance_name: String) -> Self {
        Self {
            identity,
            instance_name,
        }
    }

    /// Sign a mesh request with this instance's Ed25519 private key.
    pub fn sign_request(&self) -> Result<String, String> {
        let priv_key = self
            .identity
            .private_key
            .as_ref()
            .ok_or("No private key in identity")?;
        crown::sign_request(priv_key, &self.identity.instance_id)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn instance_mesh_identity_derives_from_instance_not_hostname() {
        assert_eq!(
            super::instance_mesh_name_for_dev("same-host", "-worktree-a"),
            "same-host [dev-worktree-a]"
        );
        assert_eq!(
            super::instance_mesh_name_for_dev("same-host [dev-worktree-a]", "-worktree-b"),
            "same-host [dev-worktree-b]"
        );
    }
}
