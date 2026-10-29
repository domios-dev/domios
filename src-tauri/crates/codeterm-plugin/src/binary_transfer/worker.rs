use super::{
    model::*,
    network::TransferNetwork,
    policy::{TransferAbort, TransferSession},
    upload,
};
use futures_util::StreamExt;
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue},
    Url,
};
use sha2::{Digest, Sha256};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU16, AtomicUsize, Ordering},
        Arc,
    },
    time::Instant,
};
static ACTIVE: AtomicUsize = AtomicUsize::new(0);
struct Permit(Arc<TransferSession>);
impl Drop for Permit {
    fn drop(&mut self) {
        ACTIVE.fetch_sub(1, Ordering::AcqRel);
        self.0.active.fetch_sub(1, Ordering::AcqRel);
    }
}
impl std::fmt::Display for TransferRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for TransferRefusal {}

pub fn execute(
    session: Arc<TransferSession>,
    request: TransferRequest,
    abort: TransferAbort,
) -> std::result::Result<TransferReceipt, TransferFailure> {
    let network = session.network();
    execute_with(session, request, abort, network)
}
pub(crate) fn execute_until(
    session: Arc<TransferSession>,
    request: TransferRequest,
    abort: TransferAbort,
    deadline: Instant,
) -> std::result::Result<TransferReceipt, TransferFailure> {
    let network = session.network();
    execute_at(session, request, abort, network, Some(deadline))
}
pub(crate) fn execute_with(
    session: Arc<TransferSession>,
    request: TransferRequest,
    abort: TransferAbort,
    network: Arc<dyn TransferNetwork>,
) -> std::result::Result<TransferReceipt, TransferFailure> {
    execute_at(session, request, abort, network, None)
}
pub(super) fn execute_at(
    session: Arc<TransferSession>,
    request: TransferRequest,
    abort: TransferAbort,
    network: Arc<dyn TransferNetwork>,
    bound: Option<Instant>,
) -> std::result::Result<TransferReceipt, TransferFailure> {
    let started = Arc::new(AtomicBool::new(false));
    let status = Arc::new(AtomicU16::new(0));
    let result = (|| {
        ACTIVE
            .try_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < 32).then_some(count + 1)
            })
            .map_err(|_| TransferRefusal::Unavailable)?;
        if session
            .active
            .try_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < 8).then_some(count + 1)
            })
            .is_err()
        {
            ACTIVE.fetch_sub(1, Ordering::AcqRel);
            return Err(TransferRefusal::Unavailable);
        }
        let _permit = Permit(session.clone());
        let requested = session.deadline(&request)?;
        let deadline = bound.map_or(requested, |bound| bound.min(requested));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| TransferRefusal::Unavailable)?;
        let result=runtime.block_on(async {
            tokio::select! {
                biased;
                _=session.cancelled()=>Err(TransferRefusal::NotAllowed),
                _=abort.cancelled()=>Err(TransferRefusal::Aborted),
                _=tokio::time::sleep_until(tokio::time::Instant::from_std(deadline))=>Err(TransferRefusal::Deadline),
                result=run(session.clone(),request,abort.clone(),network,started.clone(),status.clone(),deadline)=>result,
            }
        });
        runtime.shutdown_background();
        result
    })();
    log::info!(
        "plugin transfer audit plugin={} started={} status={} outcome={}",
        session.plugin_id(),
        started.load(Ordering::Acquire),
        status.load(Ordering::Acquire),
        match &result {
            Ok(_) => "completed",
            Err(_) => "refused_or_uncertain",
        }
    );
    result.map_err(|error| TransferFailure {
        error,
        request_may_have_started: started.load(Ordering::Acquire),
        status: match status.load(Ordering::Acquire) {
            0 => None,
            value => Some(value),
        },
    })
}
fn check(
    session: &TransferSession,
    url: &Url,
    abort: &TransferAbort,
    deadline: Instant,
) -> Result<()> {
    if abort.is_aborted() {
        return Err(TransferRefusal::Aborted);
    }
    if Instant::now() >= deadline {
        return Err(TransferRefusal::Deadline);
    }
    session.authorize(url)
}
fn request_headers(session: &TransferSession, request: &TransferRequest) -> Result<HeaderMap> {
    if request.headers.len() > 16 {
        return Err(TransferRefusal::InvalidRequest);
    }
    let mut headers = HeaderMap::new();
    for (name, value) in &request.headers {
        if forbidden_header(name)
            || (!matches!(
                name.to_ascii_lowercase().as_str(),
                "accept" | "content-type" | "idempotency-key"
            ) && !session
                .limits
                .request_headers
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(name)))
            || value.len() > 1024
        {
            return Err(TransferRefusal::NotAllowed);
        }
        headers.insert(
            HeaderName::from_bytes(name.as_bytes()).map_err(|_| TransferRefusal::InvalidRequest)?,
            HeaderValue::from_str(value).map_err(|_| TransferRefusal::InvalidRequest)?,
        );
    }
    if request.body.is_some() {
        headers.remove(reqwest::header::CONTENT_TYPE);
    }
    headers.insert(
        reqwest::header::ACCEPT_ENCODING,
        HeaderValue::from_static("identity"),
    );
    Ok(headers)
}
async fn run(
    session: Arc<TransferSession>,
    mut request: TransferRequest,
    abort: TransferAbort,
    network: Arc<dyn TransferNetwork>,
    started: Arc<AtomicBool>,
    status: Arc<AtomicU16>,
    deadline: Instant,
) -> Result<TransferReceipt> {
    if !matches!(
        request.method.as_str(),
        "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE"
    ) || (request.body.is_some() && matches!(request.method.as_str(), "GET" | "HEAD"))
    {
        return Err(TransferRefusal::InvalidRequest);
    }
    request
        .response
        .validate(session.limits.max_download_bytes)?;
    let mut url = Url::parse(&request.url).map_err(|_| TransferRefusal::InvalidRequest)?;
    check(&session, &url, &abort, deadline)?;
    let initial_origin = url.origin();
    let mut headers = request_headers(&session, &request)?;
    if let Some(credential) = &request.credential {
        session.credential_allowed(&url, credential)?;
        for (name, value) in session.authority.credentials(credential)? {
            if name.len() > 128
                || value.len() > 8192
                || matches!(
                    name.to_ascii_lowercase().as_str(),
                    "host" | "content-length" | "transfer-encoding" | "connection" | "content-type"
                )
            {
                return Err(TransferRefusal::NotAllowed);
            }
            headers.insert(
                HeaderName::from_bytes(name.as_bytes())
                    .map_err(|_| TransferRefusal::InvalidRequest)?,
                HeaderValue::from_str(&value).map_err(|_| TransferRefusal::InvalidRequest)?,
            );
        }
    }
    let has_body = request.body.is_some();
    let mut upload = upload::build(
        session.clone(),
        &url,
        request.body.take(),
        abort.clone(),
        deadline,
    )?;
    let mut response = None;
    for hop in 0..=3 {
        check(&session, &url, &abort, deadline)?;
        if url.origin() == initial_origin {
            if let Some(credential) = &request.credential {
                session.credential_allowed(&url, credential)?;
                session.authority.credentials(credential)?;
            }
        }
        codeterm_decision::physical::before_request().map_err(|_| TransferRefusal::NotAllowed)?;
        started.store(true, Ordering::Release);
        let current = network
            .request(&url, &request.method, headers.clone(), upload)
            .await?;
        status.store(current.status, Ordering::Release);
        if (300..400).contains(&current.status) {
            if hop == 3 || has_body || !matches!(request.method.as_str(), "GET" | "HEAD") {
                return Err(TransferRefusal::RedirectDenied);
            }
            let next = url
                .join(
                    current
                        .location
                        .as_deref()
                        .ok_or(TransferRefusal::RedirectDenied)?,
                )
                .map_err(|_| TransferRefusal::RedirectDenied)?;
            check(&session, &next, &abort, deadline)?;
            if next.origin() != initial_origin {
                headers = HeaderMap::new();
                headers.insert(
                    reqwest::header::ACCEPT_ENCODING,
                    HeaderValue::from_static("identity"),
                );
            }
            url = next;
            upload = upload::build(session.clone(), &url, None, abort.clone(), deadline)?;
            continue;
        }
        response = Some(current);
        break;
    }
    let mut response = response.ok_or(TransferRefusal::RedirectDenied)?;
    if !request.response.statuses().contains(&response.status) {
        return Err(TransferRefusal::HttpStatus);
    }
    match request.response {
        Output::File {
            file_name,
            content_type,
            max_bytes,
            sha256,
            ..
        } => {
            save_file(
                &session,
                &url,
                &abort,
                deadline,
                response,
                FileTarget {
                    file_name,
                    content_type,
                    max_bytes,
                    expected_sha: sha256,
                },
            )
            .await
        }
        Output::Json { max_bytes, .. } => {
            if response.content_type.as_deref() != Some("application/json") {
                return Err(TransferRefusal::MimeMismatch);
            }
            if response.length.is_some_and(|length| length > max_bytes) {
                return Err(TransferRefusal::TooLarge);
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.stream.next().await {
                let chunk = chunk?;
                check(&session, &url, &abort, deadline)?;
                if chunk.len() > upload::CHUNK
                    || bytes.len().saturating_add(chunk.len()) as u64 > max_bytes
                {
                    return Err(TransferRefusal::TooLarge);
                }
                bytes.extend_from_slice(&chunk);
            }
            if response
                .length
                .is_some_and(|length| length != bytes.len() as u64)
            {
                return Err(TransferRefusal::FileChanged);
            }
            if let Some(state) = &response.upload {
                state.finish()?;
            }
            let text = std::str::from_utf8(&bytes).map_err(|_| TransferRefusal::InvalidResponse)?;
            let data = serde_json::from_str(text).map_err(|_| TransferRefusal::InvalidResponse)?;
            check(&session, &url, &abort, deadline)?;
            Ok(TransferReceipt::Json {
                status: response.status,
                data,
                size: bytes.len() as u64,
                sha256: format!("{:x}", Sha256::digest(&bytes)),
            })
        }
        Output::Empty { .. } => {
            while let Some(chunk) = response.stream.next().await {
                if !chunk?.is_empty() {
                    return Err(TransferRefusal::InvalidResponse);
                }
                check(&session, &url, &abort, deadline)?;
            }
            if let Some(state) = &response.upload {
                state.finish()?;
            }
            check(&session, &url, &abort, deadline)?;
            Ok(TransferReceipt::Empty {
                status: response.status,
            })
        }
    }
}
struct FileTarget {
    file_name: String,
    content_type: String,
    max_bytes: u64,
    expected_sha: Option<String>,
}
async fn save_file(
    session: &TransferSession,
    url: &Url,
    abort: &TransferAbort,
    deadline: Instant,
    mut response: super::network::NetworkResponse,
    target: FileTarget,
) -> Result<TransferReceipt> {
    let FileTarget {
        file_name,
        content_type,
        max_bytes,
        expected_sha,
    } = target;
    if response.content_type.as_deref() != Some(content_type.as_str()) {
        return Err(TransferRefusal::MimeMismatch);
    }
    if response.length.is_some_and(|length| length > max_bytes) {
        return Err(TransferRefusal::TooLarge);
    }
    match std::fs::create_dir(&session.root) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(TransferRefusal::Storage),
    }
    let display = upload::safe_name(
        &file_name.chars().take(40).collect::<String>(),
        &content_type,
    )?;
    let name = format!("{}-{display}", uuid::Uuid::new_v4().simple());
    let mut target =
        codeterm_blueprint_types::owned_output::OwnedOutput::create(&session.root, &name)
            .map_err(|_| TransferRefusal::NotAllowed)?;
    let mut size = 0u64;
    let mut head = Vec::with_capacity(32);
    let mut digest = Sha256::new();
    while let Some(bytes) = response.stream.next().await {
        let bytes = bytes?;
        if bytes.len() > upload::CHUNK {
            return Err(TransferRefusal::TooLarge);
        }
        check(session, url, abort, deadline)?;
        size = size
            .checked_add(bytes.len() as u64)
            .ok_or(TransferRefusal::TooLarge)?;
        if size > max_bytes {
            return Err(TransferRefusal::TooLarge);
        }
        let keep = (32 - head.len()).min(bytes.len());
        head.extend_from_slice(&bytes[..keep]);
        digest.update(&bytes);
        target
            .write_all(&bytes)
            .map_err(|_| TransferRefusal::Storage)?;
    }
    if response.length.is_some_and(|length| length != size) {
        return Err(TransferRefusal::FileChanged);
    }
    if let Some(state) = &response.upload {
        state.finish()?;
    }
    upload::validate_mime(&content_type, &head)?;
    let sha256 = format!("{:x}", digest.finalize());
    if expected_sha
        .as_ref()
        .is_some_and(|expected| *expected != sha256)
    {
        return Err(TransferRefusal::DigestMismatch);
    }
    check(session, url, abort, deadline)?;
    target.publish().map_err(|_| TransferRefusal::Storage)?;
    check(session, url, abort, deadline)?;
    target.retain();
    Ok(TransferReceipt::File {
        status: response.status,
        file: TransferFile {
            path: name,
            file_name: display,
            content_type,
            size,
            sha256,
        },
    })
}
pub async fn execute_async(
    session: Arc<TransferSession>,
    request: TransferRequest,
    abort: TransferAbort,
) -> std::result::Result<TransferReceipt, TransferFailure> {
    let observer = codeterm_decision::physical::current();
    tokio::task::spawn_blocking(move || {
        let _scope = observer.map(codeterm_decision::physical::RequestScope::enter);
        execute(session, request, abort)
    })
    .await
    .unwrap_or_else(|_| {
        Err(TransferFailure {
            error: TransferRefusal::TransportUnconfirmed,
            request_may_have_started: true,
            status: None,
        })
    })
}
