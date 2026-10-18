use codeterm_types::server_bootstrap::ClientIdentity;
use std::{
    io,
    path::{Path, PathBuf},
};

pub fn validate_client_identity(identity: &ClientIdentity) -> io::Result<()> {
    match identity {
        ClientIdentity::Sha256 { sha256 }
            if !cfg!(target_os = "macos")
                && sha256.len() == 64
                && sha256
                    .bytes()
                    .all(|value| value.is_ascii_digit() || (b'a'..=b'f').contains(&value)) =>
        {
            Ok(())
        }
        ClientIdentity::MacosDesignatedRequirement {
            requirement,
            team_id,
            identifier,
        } if cfg!(target_os = "macos") => {
            validate_macos_requirement(requirement, team_id, identifier)
        }
        _ => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Client identity kind or value does not match this platform",
        )),
    }
}

pub fn validate_macos_requirement(
    requirement: &str,
    team_id: &str,
    identifier: &str,
) -> io::Result<()> {
    let expected = format!(
        r#"anchor apple generic and identifier "com.codeterm.app" and certificate leaf[subject.OU] = "{team_id}""#
    );
    if team_id.len() != 10
        || !team_id
            .bytes()
            .all(|value| value.is_ascii_uppercase() || value.is_ascii_digit())
        || identifier != "com.codeterm.app"
        || requirement != expected
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Client signing requirement is not constrained to the approved team and identifier",
        ));
    }
    Ok(())
}

pub fn macos_app_from_resources(resources: &Path) -> io::Result<PathBuf> {
    let resources = resources.canonicalize()?;
    let contents = resources.parent().ok_or_else(invalid_bundle)?;
    let app = contents.parent().ok_or_else(invalid_bundle)?;
    if resources.file_name().is_none_or(|name| name != "Resources")
        || contents.file_name().is_none_or(|name| name != "Contents")
        || app.file_name().is_none_or(|name| name != "CodeTerm.app")
    {
        return Err(invalid_bundle());
    }
    Ok(app.to_path_buf())
}

fn invalid_bundle() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Client resources are not in the expected app bundle",
    )
}

pub fn resolve_server_executable(
    resources: &Path,
    relative: &Path,
    identity: &ClientIdentity,
) -> io::Result<PathBuf> {
    let resources = resources.canonicalize()?;
    if matches!(identity, ClientIdentity::None) {
        let expected = if cfg!(windows) {
            "domios-server/domios-server.exe"
        } else {
            "domios-server/domios-server"
        };
        if relative != Path::new(expected) {
            return Err(invalid_bundle());
        }
        return confined_executable(&resources, relative);
    }
    validate_client_identity(identity)?;
    #[cfg(target_os = "macos")]
    {
        if relative != Path::new("../MacOS/domios-server") {
            return Err(invalid_bundle());
        }
        let app = macos_app_from_resources(&resources)?;
        let expected = app.join("Contents/MacOS/domios-server");
        if !expected.symlink_metadata()?.file_type().is_file()
            || expected.canonicalize()? != expected
        {
            return Err(invalid_bundle());
        }
        Ok(expected)
    }
    #[cfg(not(target_os = "macos"))]
    {
        confined_executable(&resources, relative)
    }
}

fn confined_executable(resources: &Path, relative: &Path) -> io::Result<PathBuf> {
    if relative.as_os_str().is_empty()
        || !relative
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
    {
        return Err(invalid_bundle());
    }
    let executable = resources.join(relative).canonicalize()?;
    if !executable.starts_with(resources) || !executable.is_file() {
        return Err(invalid_bundle());
    }
    Ok(executable)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn client_identity_kind_and_digest_are_platform_bound() {
        assert!(validate_client_identity(&ClientIdentity::None).is_err());
        let raw = ClientIdentity::Sha256 {
            sha256: "a".repeat(64),
        };
        let signed = ClientIdentity::MacosDesignatedRequirement {
            requirement: r#"anchor apple generic and identifier "com.codeterm.app" and certificate leaf[subject.OU] = "ABCDE12345""#.into(),
            team_id: "ABCDE12345".into(), identifier: "com.codeterm.app".into(),
        };
        assert_eq!(
            validate_client_identity(&raw).is_ok(),
            !cfg!(target_os = "macos")
        );
        assert_eq!(
            validate_client_identity(&signed).is_ok(),
            cfg!(target_os = "macos")
        );
        assert!(validate_client_identity(&ClientIdentity::Sha256 {
            sha256: "A".repeat(64)
        })
        .is_err());
    }
    #[test]
    fn macos_requirement_rejects_weak_or_injected_identity() {
        let requirement = r#"anchor apple generic and identifier "com.codeterm.app" and certificate leaf[subject.OU] = "ABCDE12345""#;
        assert!(validate_macos_requirement(requirement, "ABCDE12345", "com.codeterm.app").is_ok());
        for invalid in [
            r#"identifier "com.codeterm.app""#,
            "anchor apple generic",
            "true",
        ] {
            assert!(validate_macos_requirement(invalid, "ABCDE12345", "com.codeterm.app").is_err());
        }
        assert!(validate_macos_requirement(
            &format!("{requirement} or true"),
            "ABCDE12345",
            "com.codeterm.app"
        )
        .is_err());
        assert!(validate_macos_requirement(requirement, "ZZZZZ12345", "com.codeterm.app").is_err());
        assert!(validate_macos_requirement(requirement, "ABCDE12345", "com.other.app").is_err());
    }
}
