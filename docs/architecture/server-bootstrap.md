# Independent server bootstrap

The server executable is `domios-server` (`domios-server.exe` on Windows).
It owns the existing service composition, Tokio runtime, instance lifetime,
resources and shutdown. The native wrapper temporarily consumes that shared
composition while the separately owned thin-client handoff is implemented.
The standalone server installs `ProcessShell` with `NoShell` and leaves
`ApplicationHostState` absent. Native-only operations remain unavailable.

## Source and ownership map

`src-tauri/src/run.rs` service setup moved with `git mv` to
`crates/domios-server/src/composition.rs`. `startup/servers.rs`, state loading,
restore helpers, model catalogue forwarding, incidents, logging and background
services moved with their tests. The root wrapper retains only native setup,
managed adapters and desktop lifetime. The old keepawake wrappers now call the
same portable keepawake functions from composition/background services.
The removed native `ui(AppHandle)` adapter is replaced by the composition's
explicit `Arc<dyn UiEvents>`. `resume_from_disk(state, app)` now calls the core
`resume_from_disk(state, &ui)` in composition; `start_supervisor(state, app)`
calls core `start_supervisor(state, service.ui.clone())` in background startup.
Only the approved old `mod keepawake;` declaration was removed from native lib.rs.

`compose(ui_factory, shell, frontend_dev_url, resource_dir)` creates exactly one
`ComposedService { states, service, frontend_bundles }`. Its factory receives the
same `EventBroadcasterState` used by HTTP/SSE. Generic events use the existing web
redaction function; native/targeted events never become generic web events.
The standalone server has no native enrollment or installer implementation.
The native wrapper retains its existing optional desktop host during extraction.

## Pre-write authority

`ResolvedInstancePaths::current()` derives data/config from the existing
`config_path()` without creating data or calling the mesh identity loader.
Resolution handles existing ancestors, missing leaves, symlinks/junctions,
parent components and Windows case normalization for lock keys.
`try_acquire()` locks sorted data and config coordination keys; shared config
paths cannot obtain two writers through different data roots. Handles never
replace or unlink a lock inode. A loser refuses before logging, mesh UUID,
config, hook, credential or crash initialization. Only the coordination
registry/files exist before ownership. The server retains its path and UUID
locks until OS exit, including cleanup failures. The native extraction wrapper
also acquires path ownership before its logging/crash initialization.

A normal duplicate refuses. It never waits for a predecessor and cannot claim
an update handoff. Authorized replacement remains a separate continuity/update
protocol; this bootstrap supplies exact inspect and shutdown, not installer
admission or a replacement shortcut. The current build comparison is strict:
request.build must equal the intended running server build. Native-only update
compatibility must select that server build through the reviewed staged-pair
contract; client build equality is not assumed.

## Private broker and control channel

The shared data-only contract is `codeterm_types::server_bootstrap`.
`LaunchOptions`, `InstanceSelector`, `ServerIdentity`, `ProcessProvenance`,
`ServerReady`, `ServerExpectation`, `AttachRequest`, `AttachReply` and
`BootstrapOperation` are the authoritative serde types. Protocol is version 1;
API version uses `frontend_bundle_api_version::API_VERSION`.

`LaunchOptions.probe_identity_alias` defaults absent. Only an isolated
`split-oracle-*` Development launch with its exact owned sink can supply it.
Both alias paths must resolve to the ordinary data/config tuple before ownership
or writes. The server then uses the ordinary canonical tuple; an alias cannot
relocate service storage. Windows uses a probe-owned junction, Unix a symlink.
HOME/USERPROFILE environment substitution is not Windows alias evidence.

`domios-server launch` accepts one bounded `LaunchOptions` JSON line through
private inherited stdin and returns one `AttachReply` through private stdout.
It creates the exact same executable in `serve` mode with independent OS
ownership. The credential never travels in argv, URLs, logs or readiness
inventory. The optional `--hold` broker flag exists for the launcher-exit oracle.
Both bootstrap streams must be OS pipes/sockets, not a terminal or file.
Once service readiness is established, a disconnected launch observer cannot
revoke server lifetime; the launcher also stops owning failure cleanup then.

`domios-server attach` accepts an `AttachRequest` through the same protected
stdio convention. It uses local length-prefixed IPC (32 KiB maximum), owner-only
access and a pinned OS peer. The peer must be the expected server executable;
this broker is API bootstrap, not native client enrollment. Windows uses a
process handle, creation FILETIME and executable path. Linux uses pidfd plus
creation token/executable. macOS uses a process kqueue plus creation provenance
and executable. Authentication never uses public `/api/health`.

Inspect checks identity, challenge, intended build and protocol/API. A supplied
expectation also binds exact process creation provenance and epoch. Shutdown
requires that expectation. `observe` is a probe-only broker operation: after
this authenticated inspect it reports non-secret observation readiness, holds
the OS process handle, and acknowledges exact process exit within the existing
failure deadline. It grants no effect or native capability.

`serve` rejects an inherited Windows job or a Unix process that does not own its
session. Launch uses Windows breakaway/new process group/no console flags, or
Unix `setsid` with SIGHUP/SIGPIPE ignored and isolated bootstrap stdio. It never
weakens PTY kill-on-close jobs. A job that forbids breakaway produces an explicit
infrastructure failure. The launching native client must provide a permitted
launch context. SIGINT/SIGTERM or authenticated exact shutdown runs the existing
snapshot, sidecar, PTY, surface, storage, memory and keepawake teardown paths.

The ready reply follows composition, socket ownership and the existing REST
Listening watch. Restored backends and General Agent wait on a service-owned
initialization acknowledgement, never frontend generation or elapsed time.
Server path/resource provenance is explicit before the first PTY spawn. Missing
Windows ConPTY resources refuse rather than resolving through a native client.

The old `materialize_restored_backends(wait_frontend, ...)` and
`report_general_agent_frontend_deadline` helpers are removed. Both production
launch paths now use `service_initialized(Receiver<()>)`; each has its own
receiver and composition acknowledges them independently after server setup.
A disconnected sender refuses launch. `server-test-renames.txt` records three
semantic test replacements for that production receiver, not name equivalence.
The existing failure/session ownership, protected/frozen/backing filters and
restore transport-wave tests remain, plus a saved-session candidate regression.

## Worker resource and packaging contract

Build the actual `cli/Cargo.toml` `codeterm` binary for the selected host first.
Select its non-test Cargo artifact by target source `cli/src/main.rs`, then
SHA-256 its bytes. Pass that digest as `CODETERM_SERVER_CLI_SHA256` when building
the server; its build script embeds `SERVER_CLI_SHA256`. An ordinary debug/test
build may omit the digest, but that executable refuses service initialization.
There is no production fallback when the digest/resource is absent or wrong.

Porter must package that same CLI at `resources/cli/codeterm` on Unix or
`resources/cli/codeterm.exe` on Windows (the existing Windows target-qualified
resource names are also resolved by the shared resolver). Package Windows PTY
resources at `resources/conpty/{x64,arm64}/{conpty.dll,OpenConsole.exe}`.
The CLI, native client and server are distinct artifact roles.

After path ownership and before memory/service initialization, the server
verifies the selected resource using an open file, copies it into a private
`domios-worker-*` lifetime directory, and verifies the copied bytes again.
It retains that directory and a read handle through shutdown. Unix uses a
private directory and mode 0500 executable; Windows denies write/delete sharing.
Only the process-local `CODETERM_CLI_PATH` points to this lease. Replacement of
the packaged resource cannot redirect later maintenance launches. This binding
does not claim SHA alone provides executable provenance or confines a hostile
process already running as the same OS user.

The existing supervisor spawn optionally emits PID, selected path and a duplicate
kernel witness through a bounded nonblocking channel. Windows duplicates the
original child handle. Unix acquires pidfd/process-kqueue before its sole Child
has been polled or dropped, while an exited child is still unreaped. Receiver
inspection separately checks creation and executable provenance, with witness
liveness checked around PID-based reads. A macOS event handle alone is not
identity. Missing/expired evidence emits unavailable, and the probe fails that
native row. The observer never owns child wait/control or changes maintenance
retry/circuit policy. Closed/full queues drop the duplicate without blocking.
Its private bounded audit is probe-only, not readiness or authority.

## Evidence boundary

`build.sh process-probe` owns the canonical artifact selection, exact probe
feature/target closure and disposable native process oracle. Packaging must
record the shipped artifact's own release feature/target closure separately.
Its two cases and
focused commands are documented in the build/testing skills. Pure contract
tests and source guards do not prove OS lifetime or shipped closure. The native
cases must be executed on each platform and reported separately. Windows job
permission failures, missing resources and toolchain failures are infrastructure
results, not source diagnostics or passing process evidence.

Rollback is the whole server extraction slice: its crate/workspace/lockfile,
shared DTOs, path arbitration, root wrapper, moved helpers, build/probe routing,
source guards, catalogue and operational recipes. Do not revert only the root
wrapper while retaining a second state owner. Client/update/packaging changes
must consume the ancestry-preserved server merge before that rollback decision.
