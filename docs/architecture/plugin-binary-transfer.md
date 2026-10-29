# Plugin binary transfer

The backend capability `capabilities.binaryTransfer` provides bounded HTTPS transfers without converting binary data to text. It does not grant iframe networking, subprocess access, WSS, durable provider jobs, or automatic retries.

## Declaration and authority

`BinaryTransferDescriptor` declares `maxUploadBytes`, `maxDownloadBytes`, `timeoutMs`, optional `credentialOrigins`, and optional `requestHeaders`. Defaults are 16 MiB upload, 64 MiB download and 30 seconds. Hard ceilings are 256 MiB per direction and 120 seconds. A request may narrow the deadline. Physical caller deadlines also apply.

Every destination must have an exact current network grant. A hostname grant permits HTTPS port 443 only; another port needs its explicit grant. No subdomain expansion, URL credentials, fragments, proxy inheritance or private/local addresses are accepted. The resolver rejects a mixed public/private DNS answer and supplies the checked addresses directly to the connection. At most eight bounded DNS lookups may remain in flight; a system resolver operation already running cannot be forcibly interrupted.

Credentials are references resolved in the host: `{kind:"secret",name,header,prefix?}` or `{kind:"declared",id,headers}` with existing manifest credential templates. Current secrets permission and the exact network grant are required. `credentialOrigins` separately pins each `secret:<name>` or `declared:<id>` to canonical HTTPS origins. An absent pin refuses injection. No implicit instance or supporter token is supplied.

Literal request headers are limited to Accept, Content-Type, Idempotency-Key and at most sixteen declared public `requestHeaders`. Authentication, cookies and transport/framing headers cannot be declared there. Values are bounded; credential values are never returned in a receipt. Body Content-Type and Content-Length are host-composed. Body-free GET/HEAD redirects are limited to three and reauthorize each destination. Crossing an origin removes all initial custom and credential headers. Upload redirects refuse without replaying the body.

## Inputs and responses

Backend-only `TransferInput` is either `{kind:"owned",path,contentType,fileName,identity?:{size,sha256}}` or `{kind:"selected",handle}`. Owned paths are relative to the plugin data directory, opened without following symlinks/reparse points; declared credential files remain unreadable. A selected handle is one use, belongs to the current plugin runtime epoch, expires after five minutes, and is bound to one destination HTTPS origin. Native selectors accept an already-opened regular file, never a JS absolute path.

The host hashes the fixed prefix length before transmission and again as it streams. Truncation or changed bytes refuse. Growth beyond that snapshot is never sent. Durable/quoted inputs must supply their recorded size and SHA-256; keeping a descriptor alone does not make mutable bytes immutable. The durable runner owns retention and run/Team/workspace/consumer authorization, then obtains a fresh epoch grant after restart or owner hold.

A request body is a file or multipart with at most sixteen text/file parts and 64 KiB total text. There is no application/json MIME part; adapters may explicitly serialize a schema-validated JSON value into a documented text field. Files stream in 64 KiB chunks. Network packet staging is bounded at 1 MiB, not a whole-file buffer.

The request chooses an explicit response and one to eight allowed terminal HTTP statuses:

- `{kind:"file",fileName,contentType,maxBytes,sha256?,statuses}` streams to a newly created plugin-owned file, checks passive MIME signature or explicit octet-stream, hashes, and publishes exclusively after durable write. The receipt is `{kind:"file",status,file:{path,fileName,contentType,size,sha256}}`; path remains backend-only.
- `{kind:"json",maxBytes,statuses}` accepts application/json, strict UTF-8 and valid JSON under a hard 256 KiB metadata cap. Its receipt is `{kind:"json",status,data,size,sha256}`. It creates no output file.
- `{kind:"empty",statuses}` accepts no body and returns `{kind:"empty",status}`. A documented 204 acknowledgement does not imply a provider job or generation completion.

Partial files are removed on refusal/abort and never overwrite existing files. The output directory is pinned during creation, publication and cleanup. There is no crash-recovery ledger or resumable transport.

## Native and QuickJS ports

The SDK exports `HostTransfer`, `TransferRequest`, `TransferResult`, `TransferInputGrant` and related types from `binaryTransfer.ts`. `host.transfer.async(request, syncContinuation)` unwinds the VM using the existing sanctioned host-job mechanism; it is not a Promise. `start` returns `{jobId}` or a typed failure and `abort(jobId)` addresses only the exact plugin/runtime scope. This ephemeral transport id is never a provider job id.

Failures contain only `{error,requestMayHaveStarted,status}`. After dispatch, bad MIME/JSON, size, digest, deadline, source mutation or missing continuation cannot establish remote non-execution. Callers preserve uncertainty and do not replay. Provider adapters must validate a JSON job receipt and persist its real provider job identity before acquiring later outputs. Abort stops local transport; it does not claim provider cancellation. A completed receipt settles independently of iframe delivery.

Native callers use `select_plugin_input` or the two-phase `prepare_plugin_input_bounded` / `PreparedInputGrant::commit` with `FileIdentity` and `InputPreparation`. Preparation validates the current consumer epoch before and after hashing. Dropping an uncommitted preparation revokes its ticket. Deadline/abort checks occur between chunks; an already-entered kernel file operation cannot be preempted.

## Selected file producer boundary

`ingest_frame_input(plugin, frame, &verified_context, source, display_name, declared_mime, limits)` accepts an opened host-selected file, bounded bytes or a bounded/deadlined reader, copies into a confined plugin `inputs` directory, sniffs passive type, hashes and registers the exact frame handle. `ViewProvider::view_inputs()` declares accepted MIME types, maximum bytes and per-pick file count; route limits may only narrow it. The caller resolves its captured private view lease, validates the service context and passes that exact context into ingestion. A presentation namespace alone is insufficient because reload remints a new document in the same namespace. Each copy check and registration fence retains the original context; stale cleanup removes only its exact handle and cannot retire a newer document. `OpenedMedia.size` is the registered length for bounded exports. Passive images may use their exact-frame preview URL when the media delivery byte/dimension bounds pass; otherwise they remain gesture-only file chips.

`mediaFile(id,context)` is lookup-only. Media registration requests carry the captured private view lease outside iframe arguments, and require that lease to match the presentation namespace. A delayed old request cannot adopt a reminted document. Its JS adapter carries native context across an interruptible VM call, bounded by five seconds and any narrower physical deadline. Dropping its async waiter signals cancellation; VM lock admission, script execution and sanctioned host-job waits observe the bound. This does not preempt an already-entered arbitrary blocking native function or kernel I/O.

`host.transfer.bindInput(handle,destinationOrigin,syncContinuation)` captures the native trusted view-call context before unwinding. The service validates the live document/provider/owner/Team context before and after opening the ingested handle and preparing a grant. It never accepts a context or path from iframe arguments. Those checks surround grant minting, not paid HTTP execution. Picker/web ingestion is a separate host caller; transcriber opaque input and durable run inputs retain their own verified authority boundaries. Transport primitives alone do not establish a working producer integration.

## Durable run lookup

`ViewProvider::run_media_file(id, &RunMediaLookupContext)` is a separate default-refusing native port. JS adapters call the same `mediaFile` export with the closed `MediaFileContext = ViewCallContext | RunMediaLookupContext` SDK union. Durable context has origin `durable_run` and host-stamped plugin/workspace/Team/owner/run/producer-operation/producer-port/receiver-node attribution. It is Serialize-only in Rust; no iframe or receiver argument becomes that authority.

The JS adapter refuses unless a current physical observer exists and denies effects; a missing or permissive observer has no fallback. It rejects any ambient view TLS, enters a separate native lookup scope across continuations, propagates the physical observer, and denies binary transfer start/bind while looking up an artifact. It does not enter generic viewCall or grant a document, frame, lease, budget, submission or job-admission registration. The durable runner separately validates ledger/handoff/current producer identity and exact retained content before and after lookup. A stopped producer or changed source refuses; no live frame is required for durable receipt settlement.

This narrow context is attribution, not sandboxing arbitrary Rust I/O. The adapter depends on the runner's reviewed observation-only guard across legacy host bindings, including exec and local compute controls. Discovery remains unavailable until that coherent guard and the runner admission audit land; a context or physical network observer alone never proves plugin code is effect-free.
