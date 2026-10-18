use crate::{
    client_identity::{macos_app_from_resources, validate_macos_requirement},
    PinnedProcess,
};
use core_foundation::url::CFURL;
use security_framework::os::macos::code_signing::{
    Flags, GuestAttributes, SecCode, SecRequirement, SecStaticCode,
};
use std::{io, path::Path};

pub fn verify_macos_client(
    process: &PinnedProcess,
    resources: &Path,
    requirement: &str,
    team_id: &str,
    identifier: &str,
) -> io::Result<()> {
    validate_macos_requirement(requirement, team_id, identifier)?;
    let before = process.provenance()?;
    let app = macos_app_from_resources(resources)?;
    let executable = before.executable.canonicalize()?;
    if executable.parent() != Some(app.join("Contents/MacOS").as_path()) {
        return Err(refused());
    }
    let requirement: SecRequirement = requirement.parse().map_err(|_| refused())?;
    let mut attributes = GuestAttributes::new();
    attributes.set_pid(i32::try_from(before.pid).map_err(|_| refused())?);
    let dynamic = SecCode::copy_guest_with_attribues(None, &attributes, Flags::NONE)
        .map_err(|_| refused())?;
    dynamic
        .check_validity(Flags::NONE, &requirement)
        .map_err(|_| refused())?;
    let guest_path = dynamic
        .path(Flags::NONE)
        .map_err(|_| refused())?
        .to_path()
        .ok_or_else(refused)?
        .canonicalize()?;
    if guest_path != app && guest_path != executable {
        return Err(refused());
    }
    let app_url = CFURL::from_path(&app, true).ok_or_else(refused)?;
    let installed = SecStaticCode::from_path(&app_url, Flags::NONE).map_err(|_| refused())?;
    installed
        .check_validity(
            Flags::STRICT_VALIDATE | Flags::CHECK_NESTED_CODE | Flags::CHECK_ALL_ARCHITECTURES,
            &requirement,
        )
        .map_err(|_| refused())?;
    if process.provenance()? != before {
        return Err(refused());
    }
    process.ensure_alive()
}

fn refused() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Client code signature or installed app seal does not match the approved identity",
    )
}
