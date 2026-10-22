use super::{
    process::{verify_peer, PinnedProcess},
    protocol::{
        validate_attach, validate_reply, AttachReply, AttachRequest, BootstrapOperation,
        ServerReady, FAILURE_DEADLINE,
    },
};
use interprocess::local_socket::{
    tokio::{prelude::*, Listener, Stream},
    GenericNamespaced, ListenerOptions,
};
use sha2::{Digest, Sha256};
use std::{io, path::Path, sync::Arc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug)]
struct EndpointUnavailable(io::Error);

impl std::fmt::Display for EndpointUnavailable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Bootstrap endpoint is unavailable")
    }
}

impl std::error::Error for EndpointUnavailable {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

pub fn helper_exit_code(error: &io::Error) -> i32 {
    if error
        .get_ref()
        .is_some_and(|inner| inner.is::<EndpointUnavailable>())
    {
        codeterm_types::server_bootstrap::BOOTSTRAP_ATTACH_UNAVAILABLE_EXIT_CODE
    } else if error.kind() == io::ErrorKind::AlreadyExists {
        3
    } else {
        1
    }
}

pub(crate) fn classify_connect_error(
    request: &AttachRequest,
    intended_build: &str,
    error: io::Error,
) -> io::Error {
    if request.operation == BootstrapOperation::Inspect
        && request.expected.is_none()
        && request.protocol == super::protocol::BOOTSTRAP_PROTOCOL
        && request.api_version == codeterm_types::server_bootstrap::API_VERSION
        && request.build == intended_build
        && !request.challenge.is_empty()
        && request.challenge.len() <= 128
        && matches!(
            error.kind(),
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
        )
    {
        io::Error::new(io::ErrorKind::NotConnected, EndpointUnavailable(error))
    } else {
        error
    }
}

pub fn address(identity: &super::protocol::ServerIdentity) -> io::Result<String> {
    let bytes = serde_json::to_vec(identity).map_err(io::Error::other)?;
    let digest = format!("{:x}", Sha256::digest(bytes));
    Ok(format!(
        "domios-bootstrap-{}.sock",
        &digest[..codeterm_types::native_host::BOOTSTRAP_IDENTITY_HEX_LEN]
    ))
}

pub fn bind(identity: &super::protocol::ServerIdentity) -> io::Result<Listener> {
    let address = address(identity)?;
    codeterm_util::local_endpoint::prepare_namespaced(&address)?;
    let target = codeterm_util::local_endpoint::display_name(&address);
    let name = address
        .to_ns_name::<GenericNamespaced>()
        .map_err(|error| codeterm_util::local_endpoint::bind_error(&target, error))?;
    let options = ListenerOptions::new().name(name);
    // Interprocess reports socket fchmod as Unsupported on macOS; only Linux pins the mode.
    #[cfg(target_os = "linux")]
    let options = {
        use interprocess::os::unix::local_socket::ListenerOptionsExt;
        options.mode(0o600)
    };
    #[cfg(windows)]
    let options = {
        use interprocess::os::windows::{
            local_socket::ListenerOptionsExt, security_descriptor::SecurityDescriptor,
        };
        let sddl = widestring::U16CString::from_str("D:P(A;;GA;;;OW)").map_err(io::Error::other)?;
        options.security_descriptor(SecurityDescriptor::deserialize(&sddl)?)
    };
    options
        .create_tokio()
        .map_err(|error| codeterm_util::local_endpoint::bind_error(&target, error))
}

pub struct ShutdownCompletion {
    stream: Stream,
    peer: PinnedProcess,
    reply: AttachReply,
}

impl ShutdownCompletion {
    pub async fn complete(self, cleanup: io::Result<()>) -> io::Result<()> {
        after_successful_cleanup(cleanup, || async move {
            self.peer.ensure_alive()?;
            let mut stream = self.stream;
            tokio::time::timeout(FAILURE_DEADLINE, write_frame(&mut stream, &self.reply))
                .await
                .map_err(|_| {
                    io::Error::new(io::ErrorKind::TimedOut, "Shutdown completion deadline")
                })?
        })
        .await
    }
}

pub(crate) async fn after_successful_cleanup<T, F, Fut>(
    cleanup: io::Result<()>,
    send: F,
) -> io::Result<T>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = io::Result<T>>,
{
    cleanup?;
    send().await
}

pub async fn serve(
    listener: Listener,
    ready: ServerReady,
    token: Option<String>,
) -> io::Result<ShutdownCompletion> {
    let ready = Arc::new(ready);
    let (shutdown, mut stopped) = tokio::sync::mpsc::channel(1);
    loop {
        let stream = tokio::select! {
            result = listener.accept() => result?,
            completion = stopped.recv() => return completion.ok_or_else(|| io::Error::other("Shutdown admission channel closed")),
        };
        let ready = ready.clone();
        let token = token.clone();
        let shutdown = shutdown.clone();
        tokio::spawn(async move {
            if let Ok(Ok(Some(completion))) =
                tokio::time::timeout(FAILURE_DEADLINE, reply(stream, &ready, token)).await
            {
                let _ = shutdown.try_send(completion);
            }
        });
    }
}

async fn reply(
    mut stream: Stream,
    ready: &ServerReady,
    token: Option<String>,
) -> io::Result<Option<ShutdownCompletion>> {
    let peer = pin_peer(&stream)?;
    verify_peer(&peer, &ready.process.executable)?;
    let request: AttachRequest = read_frame(&mut stream).await?;
    validate_attach(&request, ready)?;
    peer.ensure_alive()?;
    let response = AttachReply {
        challenge: request.challenge,
        ready: ready.clone(),
        api_token: if request.operation == BootstrapOperation::Inspect {
            token
        } else {
            None
        },
    };
    if request.operation == BootstrapOperation::Shutdown {
        return Ok(Some(ShutdownCompletion {
            stream,
            peer,
            reply: response,
        }));
    }
    write_frame(&mut stream, &response).await?;
    Ok(None)
}

pub async fn attach(
    request: &AttachRequest,
    executable: &Path,
) -> io::Result<(AttachReply, PinnedProcess)> {
    tokio::time::timeout(FAILURE_DEADLINE, async {
        let address = address(&request.identity)?;
        let mut stream = Stream::connect(address.to_ns_name::<GenericNamespaced>()?)
            .await
            .map_err(|error| classify_connect_error(request, env!("BUILD_SHA"), error))?;
        let peer = pin_peer(&stream)?;
        let provenance = verify_peer(&peer, executable)?;
        write_frame(&mut stream, request).await?;
        let reply = read_frame(&mut stream).await?;
        validate_reply(request, &reply, &provenance)?;
        if request.operation == BootstrapOperation::Inspect {
            peer.ensure_alive()?;
        }
        Ok((reply, peer))
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Bootstrap attach deadline"))?
}

fn pin_peer(stream: &Stream) -> io::Result<PinnedProcess> {
    let credentials = stream.peer_creds()?;
    #[cfg(unix)]
    if credentials.euid() != Some(unsafe { libc::geteuid() }) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Foreign bootstrap user",
        ));
    }
    #[cfg(not(target_os = "macos"))]
    let pid = credentials
        .pid()
        .ok_or_else(|| io::Error::other("Bootstrap peer PID unavailable"))? as u32;
    #[cfg(target_os = "macos")]
    let pid = {
        use std::os::fd::{AsFd, AsRawFd};
        let Stream::UdSocket(socket) = stream;
        let mut pid: libc::pid_t = 0;
        let mut len = std::mem::size_of_val(&pid) as libc::socklen_t;
        if unsafe {
            libc::getsockopt(
                socket.as_fd().as_raw_fd(),
                libc::SOL_LOCAL,
                libc::LOCAL_PEERPID,
                (&mut pid as *mut libc::pid_t).cast(),
                &mut len,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        pid as u32
    };
    PinnedProcess::open(pid)
}

async fn read_frame<T: serde::de::DeserializeOwned>(stream: &mut Stream) -> io::Result<T> {
    let count = stream.read_u32().await? as usize;
    if count == 0 || count > 32768 {
        return Err(io::Error::other("Invalid bootstrap frame length"));
    }
    let mut bytes = vec![0; count];
    stream.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

async fn write_frame<T: serde::Serialize>(stream: &mut Stream, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    if bytes.len() > 32768 {
        return Err(io::Error::other("Bootstrap frame too large"));
    }
    stream.write_u32(bytes.len() as u32).await?;
    stream.write_all(&bytes).await?;
    stream.flush().await
}
