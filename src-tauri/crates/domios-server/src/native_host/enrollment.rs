use codeterm_types::{
    native_host::{
        NativeHostChallenge, NativeHostEnrollment, NativeHostEnrollmentRequest,
        NativeHostRejection,
        NATIVE_HOST_PROTOCOL,
    },
    server_bootstrap::{
        ProcessProvenance, ServerComponentManifest, ServerReady, BOOTSTRAP_PROTOCOL, API_VERSION,
        SERVER_COMPONENT_SCHEMA,
    },
};
use crate::bootstrap::process::PinnedProcess;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::File,
    io::{self, Read},
    path::Path,
};

pub(crate) struct EnrollmentPolicy {
    pub challenge: NativeHostChallenge,
    pub client_sha256: String,
    pub allowed_view_origins: HashSet<String>,
}

pub(crate) struct VerifiedClientProcess(ProcessProvenance);

pub(crate) fn verify_component(
    manifest: &ServerComponentManifest,
    ready: &ServerReady,
) -> io::Result<()> {
    if manifest.schema != SERVER_COMPONENT_SCHEMA
        || manifest.api_version != API_VERSION
        || manifest.bootstrap_protocol != BOOTSTRAP_PROTOCOL
        || manifest.server_build != ready.build
        || ready.api_version != manifest.api_version
        || ready.protocol != manifest.bootstrap_protocol
        || manifest.server_executable.canonicalize()? != ready.process.executable
        || !valid_digest(&manifest.server_sha256)
        || !valid_digest(&manifest.client_sha256)
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Native host component record does not match the running server",
        ));
    }
    let server_digest = sha256_file(&manifest.server_executable)?;
    if server_digest != manifest.server_sha256 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Installed server image differs from its component record",
        ));
    }
    Ok(())
}

pub(crate) fn verify_client_process(
    process: &PinnedProcess,
    policy: &EnrollmentPolicy,
) -> io::Result<VerifiedClientProcess> {
    if !valid_digest(&policy.client_sha256) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Client artifact digest is invalid",
        ));
    }
    process.ensure_alive()?;
    let provenance = process.provenance()?;
    let image = process_image_path(&provenance);
    let digest = sha256_file(&image)?;
    if digest != policy.client_sha256 || process.provenance()? != provenance {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Native host process does not match the installed client artifact",
        ));
    }
    process.ensure_alive()?;
    Ok(VerifiedClientProcess(provenance))
}

pub(crate) fn enroll(
    policy: &EnrollmentPolicy,
    request: NativeHostEnrollmentRequest,
    client_process: VerifiedClientProcess,
) -> Result<NativeHostEnrollment, NativeHostRejection> {
    let connection = &policy.challenge.connection;
    if policy.challenge.protocol != NATIVE_HOST_PROTOCOL
        || request.protocol != NATIVE_HOST_PROTOCOL
    {
        return Err(NativeHostRejection::ProtocolMismatch);
    }
    if request.challenge != policy.challenge.challenge || request.connection != *connection {
        return Err(NativeHostRejection::Unauthorized);
    }
    if request.view.as_ref().is_some_and(|view| {
        !view.is_well_formed() || !policy.allowed_view_origins.contains(&view.origin)
    }) {
        return Err(NativeHostRejection::InvalidView);
    }
    Ok(NativeHostEnrollment {
        protocol: NATIVE_HOST_PROTOCOL,
        challenge: policy.challenge.challenge.clone(),
        connection: connection.clone(),
        client_process: client_process.0,
        view: request.view,
    })
}

fn valid_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn process_image_path(provenance: &ProcessProvenance) -> std::path::PathBuf {
    #[cfg(target_os = "linux")]
    {
        std::path::PathBuf::from(format!("/proc/{}/exe", provenance.pid))
    }
    #[cfg(not(target_os = "linux"))]
    {
        provenance.executable.clone()
    }
}
