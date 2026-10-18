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

pub fn address(identity: &super::protocol::ServerIdentity) -> io::Result<String> {
    let bytes = serde_json::to_vec(identity).map_err(io::Error::other)?;
    Ok(format!("domios-bootstrap-{:x}.sock", Sha256::digest(bytes)))
}

pub fn bind(identity: &super::protocol::ServerIdentity) -> io::Result<Listener> {
    let address = address(identity)?;
    let name = address.to_ns_name::<GenericNamespaced>()?;
    let options = ListenerOptions::new().name(name);
    #[cfg(unix)]
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
    options.create_tokio()
}

pub async fn serve(
    listener: Listener,
    ready: ServerReady,
    token: Option<String>,
) -> io::Result<()> {
    let ready = Arc::new(ready);
    let (shutdown, mut stopped) = tokio::sync::mpsc::channel(1);
    loop {
        let stream = tokio::select! {
            result = listener.accept() => result?,
            _ = stopped.recv() => return Ok(()),
        };
        let ready = ready.clone();
        let token = token.clone();
        let shutdown = shutdown.clone();
        tokio::spawn(async move {
            if let Ok(Ok(true)) =
                tokio::time::timeout(FAILURE_DEADLINE, reply(stream, &ready, token)).await
            {
                let _ = shutdown.send(()).await;
            }
        });
    }
}

async fn reply(mut stream: Stream, ready: &ServerReady, token: Option<String>) -> io::Result<bool> {
    let peer = pin_peer(&stream)?;
    verify_peer(&peer, &ready.process.executable)?;
    let request: AttachRequest = read_frame(&mut stream).await?;
    validate_attach(&request, ready)?;
    peer.ensure_alive()?;
    write_frame(
        &mut stream,
        &AttachReply {
            challenge: request.challenge,
            ready: ready.clone(),
            api_token: token,
        },
    )
    .await?;
    Ok(request.operation == BootstrapOperation::Shutdown)
}

pub async fn attach(
    request: &AttachRequest,
    executable: &Path,
) -> io::Result<(AttachReply, PinnedProcess)> {
    tokio::time::timeout(FAILURE_DEADLINE, async {
        let address = address(&request.identity)?;
        let mut stream = Stream::connect(address.to_ns_name::<GenericNamespaced>()?).await?;
        let peer = pin_peer(&stream)?;
        let provenance = verify_peer(&peer, executable)?;
        write_frame(&mut stream, request).await?;
        let reply = read_frame(&mut stream).await?;
        validate_reply(request, &reply, &provenance)?;
        peer.ensure_alive()?;
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
