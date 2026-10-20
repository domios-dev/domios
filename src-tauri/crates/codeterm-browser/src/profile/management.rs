use super::{sanitize_profile_name, ProfileKind, ProfileLayout};

pub fn validate_named_profile(raw: &str) -> Result<&str, String> {
    if raw.is_empty() || sanitize_profile_name(raw) != raw {
        return Err("use a profile name of at most 40 letters, digits, - or _".into());
    }
    if ["clean", "attached"]
        .iter()
        .any(|reserved| raw.eq_ignore_ascii_case(reserved))
    {
        return Err(format!("profile name {raw} is reserved"));
    }
    Ok(raw)
}

impl ProfileLayout {
    pub fn require_named(&self, raw: &str) -> Result<std::path::PathBuf, String> {
        let name = validate_named_profile(raw)?;
        let path = self.dir_for(&ProfileKind::Named { name: name.into() });
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|_| format!("profile {name} does not exist; ask the user to create it"))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(format!("profile {name} must be a real profile directory"));
        }
        Ok(path)
    }

    pub fn create_named(&self, raw: &str) -> Result<(), String> {
        let name = validate_named_profile(raw)?;
        std::fs::create_dir_all(self.profiles_dir()).map_err(|error| error.to_string())?;
        std::fs::create_dir(self.profiles_dir().join(name))
            .map_err(|error| format!("could not create profile {name}: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selecting_a_missing_profile_does_not_create_it() {
        let root = tempfile::tempdir().unwrap();
        let layout = ProfileLayout::new(root.path().to_path_buf());
        assert!(layout.require_named("missing").is_err());
        assert!(!layout.profiles_dir().exists());
        layout.create_named("Work").unwrap();
        assert!(layout.require_named("Work").is_ok());
        assert!(layout.create_named("Work").is_err());
    }

    #[test]
    fn profile_management_rejects_reserved_and_transformed_names() {
        for name in [
            "",
            "clean",
            "Clean",
            "attached",
            "NUL",
            "../work",
            "work space",
            " work",
        ] {
            assert!(validate_named_profile(name).is_err(), "{name}");
        }
        assert!(validate_named_profile(&"a".repeat(41)).is_err());
        assert_eq!(validate_named_profile("Work-2_dev"), Ok("Work-2_dev"));
    }

    #[tokio::test]
    async fn deleting_an_inactive_profile_keeps_other_profiles() {
        let root = tempfile::tempdir().unwrap();
        let layout = ProfileLayout::new(root.path().to_path_buf());
        layout.create_named("DeleteMe").unwrap();
        layout.create_named("KeepMe").unwrap();
        super::super::delete_named_profile(&layout, "DeleteMe")
            .await
            .unwrap();
        assert!(layout.require_named("DeleteMe").is_err());
        assert!(layout.require_named("KeepMe").is_ok());
        assert!(super::super::delete_named_profile(&layout, "Missing")
            .await
            .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_profile_symlink_is_not_selectable() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let layout = ProfileLayout::new(root.path().to_path_buf());
        std::fs::create_dir_all(layout.profiles_dir()).unwrap();
        std::os::unix::fs::symlink(outside.path(), layout.profiles_dir().join("Linked")).unwrap();
        assert!(layout.require_named("Linked").is_err());
    }
}
