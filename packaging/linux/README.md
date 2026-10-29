# Headless Linux server

Use a clean Linux VM with systemd, SSH and no desktop session or Secret Service.
The primary access path uses the owner's Domios account and an explicitly paired
Mesh connection from the desktop. A dedicated Linux user isolates the server's
data and provider credentials from other users. A dedicated **Domios account on
a paid plan** (A1) remains the alternative for independent Neighbor receiving:
administer that account through SSH and the CLI, without Mesh. Paid access is
required for Neighbor messages in either mode. Same-account Mesh pairing does
not require Neighbor enrollment; enrolling this server on the desktop's account
takes receiving away from the desktop. Only do that when the owner intends it.

## Install

Install a verified Linux release resource bundle under `/opt/domios`, preserving
its layout: `domios-server/domios-server`, `cli/codeterm` and all sibling resources.
The server must have been built with its matching CLI digest and embeddings
enabled. A server with a missing or mismatched CLI resource refuses startup.
Keep the bundle root-owned and non-writable by the service user. Install the
bundle's declared runtime libraries; no WebKit, display server or keyring daemon
is required by the server. Keep the release SHA and resource digest receipt with
the installation.

```bash
sudo useradd --system --user-group --create-home --home-dir /var/lib/domios-server --shell /bin/bash domios-server
sudo install -d -m 0700 -o domios-server -g domios-server /var/lib/domios-server/.domios
sudo install -d -m 0700 -o domios-server -g domios-server /var/lib/domios-server/backups
sudo install -m 0644 packaging/linux/domios-server.service /etc/systemd/system/domios-server.service
```

Create `/var/lib/domios-server/.domios/config.toml` as `domios-server`, mode 0600:

```toml
[neighbors]
enabled = true
key_store = "file"

[memory.embedder]
provider = "local"
remote_opt_in = false
```

The explicit file-store exception (A2) stores unencrypted Neighbor private keys
in `.domios/neighbor-keys/`, inside a mode 0700 directory and mode 0600 files.
It is available only on Linux and only when Secret Service cannot be reached.
An available OS store always wins; a locked store refuses access. Missing
configuration never selects files. Wider file permissions, foreign owners,
symlinks and hard links refuse access. See `domios docs NEIGHBORS`.

The local candle embedder computes vectors on this server (A3). Provision the
chosen model's complete Hugging Face cache, including its revision reference,
into `/var/lib/domios-server/models`, owned by the service user, **before startup**.
Use the same model and revision as the release. The default is
`sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2`; required files are
`config.json`, `special_tokens_map.json`, `tokenizer_config.json`, `tokenizer.json`
and `model.safetensors`. Stage them on a connected provisioning machine. The unit
sets `HF_HUB_OFFLINE=1`: missing files fail model loading instead of downloading.
Remote embedding is disabled; account authentication and Neighbors still need
their normal network access.

```bash
sudo systemd-analyze verify /etc/systemd/system/domios-server.service
sudo systemctl daemon-reload
sudo systemctl enable --now domios-server
sudo systemctl status domios-server
```

The data directory is `/var/lib/domios-server/.domios`, selected by the dedicated
user's HOME. Production startup deliberately discards inherited data/config
overrides. `Type=forking` supervises the broker-confirmed server PID written by
`--pid-file`; it does not mistake the short-lived launcher for the server.
systemd waits for the launcher readiness acknowledgement, restarts failures,
and signals the service cgroup on stop. It removes the runtime PID file on stop.

## Open the server interface

The complete headless release archive includes the daemon-served Web UI in
`dist/` (`index.html` plus its referenced assets). Keep the entire resource tree
beside the server and CLI. A lone server executable cannot serve the interface.
Production serves it at `http://127.0.0.1:7685/` on the same origin as `/api/health`;
the API token is required for browser API access. The default API bind remains
loopback. No public `0.0.0.0` bind is needed for either access path.

### Primary: own account and Mesh

Sign the server into the owner's account and join it and the desktop to the same
Tailscale tailnet. Domios opens additional health-verified listeners on its
assigned Tailscale IPv4/IPv6 addresses. A Tailscale-only VPS needs a working
Tailscale installation, reachability between tailnet members, and an explicit
firewall allowance for TCP 7685 **on the Tailscale interface only**. Keep public
interfaces closed. Tailscale Serve HTTPS is optional; it must forward to the
loopback API and pass Domios's health/build probe before advertisement.

Complete the same-account Mesh pairing approval on both machines. On the
desktop Mesh canvas select this server, then **Web UI > Open**. The default Open
action opens the desktop's local Web UI with its local token in a fragment and
the paired peer ID. Browser requests use the signed Mesh relay; the remote token
is never sent to the browser. The server's tabs appear as remote tabs, qualified
by that peer, with its normal peer access policy. See `domios docs MESH`,
"Web UI discovery and relay". A missing Tailscale listener, pairing approval or
reachable endpoint must be resolved before Open can reach the server.

### Alternative: dedicated account and SSH

Keep the default loopback bind and open a tunnel from the owner's machine:

```bash
ssh -N -o ExitOnForwardFailure=yes -L 127.0.0.1:17685:127.0.0.1:7685 server
```

In a second local terminal, ask the server's CLI to render a URL for the tunnel's
local origin. No literal token appears in this command or shell history:

```bash
ssh -t server 'sudo -iu domios-server /opt/domios/cli/codeterm web link --host 127.0.0.1 --port 17685'
```

Open the resulting `http://127.0.0.1:17685/#token=...` link on the owner's machine.
The fragment does not go into proxy/access logs, and the Web UI removes it from
browser history after reading it. Treat the displayed link as a credential;
keep it out of chats, saved command lines and shared logs. Do not use `--query`.
When a tool needs only the token, read it through `domios system api-token` in
the clean service-user shell and pass it via stdin or a protected temporary file;
never put the token in an argv flag. This path needs no Mesh pairing or shared
Domios account. Substitute a different local port if 17685 is occupied.

For administration, enter a clean service-user login shell so inherited Domios
tab credentials and connection selectors cannot target another instance:

```bash
sudo -iu domios-server
export PATH=/opt/domios/cli:/usr/local/bin:/usr/bin:/bin
alias domios=/opt/domios/cli/codeterm
domios identify
domios account login --no-browser
```

Open the printed authorization URL on another machine. After sign-in, its
browser may show a failed `http://127.0.0.1:PORT/callback?...` navigation. Copy
**the whole URL**, including code and state, into the waiting CLI's stdin.
Bare codes are refused. The existing state, verifier and single-use checks
remain in force. EOF on stdin leaves the daemon's flow pending until timeout.
In a Domios tab, a later `domios account login --no-browser --code-from-pad callback.txt`
completes that same pending flow from the tab's pad without starting another.
Callback values are secrets: keep them out of
shell history, logs and memory. Credential persistence uses the existing
protected account credential store; the Neighbor file store is separate.

If the browser does not retain the failed URL, forward its exact callback port
before allowing the browser redirect: `ssh -N -L PORT:127.0.0.1:PORT SERVER`.
The authorization URL's `redirect_uri` contains PORT. The normal loopback
callback then completes the same pending sign-in. Do not change its redirect,
state or challenge. If the CLI is still waiting for stdin, interrupt it and
confirm completion with `domios account status`.

Read status, then explicitly enroll using its current revision:

```bash
domios account neighbors status
domios account neighbors enroll --confirm-takeover --expected-receiver-revision 0
domios account neighbors own-keys
```

Replace `0` with the observed `revision`. A stale-revision refusal requires a
fresh status and owner confirmation. Grants and concierge setup remain explicit.

## Concierge provider login

Install the chosen provider's official CLI as a root-managed binary available
to the service user. For example, provision the official Codex CLI, inspect
`codex login --help`, and run the following **as domios-server**:

```bash
codex login --device-auth
codex login status
```

Open the device URL on another machine and enter its code. Enable device-code
authentication in the provider account when required by that CLI. This login
belongs to the concierge's provider account, independently of Domios account
login. Keep its credential files under the dedicated user's HOME. Choose a
provider supporting the requested concierge sandbox and verify its login status
before creating the concierge. Do not copy the desktop user's credential store.

## Backup and restore

Run as the service user. Backup is filesystem-local and works while the daemon
is writing. It uses SQLite's online backup API for `mem/knowledge.db`, then
copies `mem/blobs/` and `storage/` into a new timestamped directory with a SHA-256
manifest. An incomplete archive is never published. The output must be outside
the data directory. Filesystem copies are not a cross-store transaction: quiesce
other file writers if an application-level snapshot across all stores is needed.
This is a memory/storage archive; configuration, account credentials, Neighbor
keys, provider credentials and model caches need their own protected recovery
procedure. Copy completed archives off the VM using encrypted storage.

```bash
domios mem backup create --out /var/lib/domios-server/backups
```

For scheduled online backups, create a Domios cron as the service account;
choose an installed provider and use its own Domios tab when reports are wanted:

```bash
domios cron create memory-backup --schedule "0 3 * * *" --tz UTC --task "Run domios mem backup create --out /var/lib/domios-server/backups once. Report the resulting archive path or error. Never restore or delete archives."
```

Stop systemd first, then run restore in the service-user shell. It acquires the
same filesystem ownership locks as the daemon and holds them through replacement;
an active or starting daemon refuses restore, even when its HTTP API is down.
Manifest and SQLite integrity checks precede replacement. Both `mem/` and
`storage/` are replaced, removing stale WAL/SHM files and caches with the old
tree. The result prints a private `.before-restore-*` directory retaining the
previous trees for recovery. Keep it until the restored instance is verified.
Existing `domios mem restore ITEM_ID` still restores a single item's history.

```bash
sudo systemctl stop domios-server
sudo -iu domios-server /opt/domios/cli/codeterm mem backup restore --from /var/lib/domios-server/backups/ARCHIVE
sudo systemctl start domios-server
```

For a Postgres backend, use that backend's `pg_dump`/`pg_restore`, not the SQLite
archive command. With connection settings supplied through a protected service
file and `.pgpass` (never an argv password):

```bash
pg_dump --format=custom --file=memory.dump --dbname=service=domios-memory
sudo systemctl stop domios-server
pg_restore --clean --if-exists --exit-on-error --single-transaction --dbname=service=domios-memory memory.dump
sudo systemctl start domios-server
```

Provision the required pgvector extension and compatible PostgreSQL tools before
restore. Back up `storage/` alongside the dump while its writers are quiesced.
Confirm the configured backend before choosing a recipe.

## Clean VM drill

The owner or Orchestrator supplies a disposable VM (O2), the verified release
bundle at `/opt/domios`, a prepared offline model cache, and the dedicated paid
test account. Run:

```bash
sudo env DOMIOS_DRILL_CLEAN_VM=1 DOMIOS_MODEL_CACHE=/path/to/prepared-cache bash scripts/headless/linux-drill.sh
```

The script refuses an existing service user, state directory, unit or occupied
port; it never targets an owner's existing instance. It installs this unit,
checks its syntax when available, starts and verifies health, signs in through
stdin, enrolls and inspects file keys, backs up while running, proves live
restore refusal, stops, restores and verifies bytes, then restarts and reads the
saved test item. Each step prints PASS or FAIL; failures stop the drill. Logs,
archives and the stopped/running test installation remain for diagnosis.
This script is an acceptance procedure; shipping it does not establish a VM PASS.

The SSH step prints SKIP unless `DOMIOS_DRILL_SSH_TARGET` names a real test SSH
route back to that VM with noninteractive fixture authentication and a trusted
host key. When supplied, the script opens an acknowledged tunnel, compares the
entry page and health boot/build identity, and checks authenticated API access
through it. This is a tunnel transport check, not a visual browser acceptance.

## Signed headless updates

The systemd-managed server discovers a headless feed instead of desktop packages.
`domios update check --force` reports a signed headless offer, or the explicit
`SignedHeadlessFeedMissing` discovery error. `domios update install` (also with
`--auto`) refuses a missing feed with JSON code `signed_headless_feed_missing`.
No archive is installed using a same-source checksum alone. Publication of these
feeds is deferred until 1.17.0 has shipped and the release Team approves the
pipeline change. The drill prints SKIP until a signed feed exists.

For a fresh installation supporting updates, extract the entire verified bundle
into `/opt/domios-releases/initial`, keep every directory/file root-owned and
non-writable by group/others, and make `/opt/domios` a root-owned symlink to that
slot. Record the exact verified bundle version as a single line in
`/opt/domios-releases/initial/.domios-version` (root-owned, mode 0644). There must
be no symlinks inside a bundle. Install the helper from that same verified build
as a separate root-owned executable; it remains outside the switched bundle:

```bash
sudo install -d -m 0755 /opt/domios-releases /usr/local/libexec
sudo ln -s /opt/domios-releases/initial /opt/domios
sudo install -m 0755 /opt/domios/domios-server/domios-server /usr/local/libexec/domios-update
sudo install -d -m 0755 /var/lib/domios-update
sudo install -m 0644 packaging/linux/domios-update.service /etc/systemd/system/domios-update.service
sudo install -m 0644 packaging/linux/49-domios-update.rules /etc/polkit-1/rules.d/49-domios-update.rules
sudo systemd-analyze verify /etc/systemd/system/domios-update.service
sudo systemctl daemon-reload
```

The polkit rule grants only `start` of `domios-update.service` to `domios-server`.
It grants neither restart/stop of other units nor unit-file management. The
server sends no version, URL, path, environment or verification result to the
helper. Feed overrides and desktop continuation are refused on this path.
Protect the helper, units, rule, release slots and all their parent directories
from service-user writes. The installed helper must be from a build containing
this updater; installing an older server binary as the helper is unsupported.

The root oneshot independently fetches the latest stable release and its signed
feed using the public updater key embedded from `tauri.conf.json`. It downloads
into a root-owned 0700 temporary directory, verifies the signed size and SHA-256,
and extracts only regular payload files into a neighboring release slot. Links,
path escapes, duplicate files, special modes, oversized payloads, missing bundle
resources and mismatched component/CLI digests are refused before activation.
Downgrades require `allow_downgrade: true` inside the signed manifest.

Activation atomically replaces the `/opt/domios` symlink, preserving both slots.
The helper restarts `domios-server.service`, waits for its startup event, and
checks `/api/health` against the expected version and build. Failed restart or
health checks restore the old symlink, restart the old bundle and verify its
health. A root-owned pending record also drives recovery through `ExecStopPost`
if the helper is interrupted. Download/verification/staging failures leave the
active bundle and running process untouched. If systemd or the old bundle cannot
recover, the result explicitly reports `rollback_failed`; operator repair is
required. The updater never deletes retained release slots.

The install response `status: scheduled` means systemd accepted the job. Read
`/api/update/status`, `/var/lib/domios-update/result.json`, or
`journalctl -u domios-update.service` for its final outcome. A successful result
records the new version and both bundle paths. Keep the previous slot until the
updated server has passed the VM drill. The root helper requires the default
loopback API address and port (`127.0.0.1:7685`).

### Feed contract for the release Team

At the immutable release URL publish
`domios-headless-linux-x86_64.json` (and `domios-headless-linux-aarch64.json` when
that archive exists). Each file is an envelope with exactly two string fields:
`manifest` (base64 of the exact UTF-8 JSON bytes) and `signature` (base64 of the
four-line minisign signature over those decoded bytes). Use the existing release
minisign private key, not the dedicated frontend key. The decoded object has
exactly these required fields:

```json
{
  "schema": 1,
  "platform": "linux-x86_64",
  "version": "1.18.0",
  "archive": "domios-headless-v1.18.0-linux-x86_64.tar.gz",
  "url": "https://github.com/domios-dev/domios/releases/download/v1.18.0/domios-headless-v1.18.0-linux-x86_64.tar.gz",
  "sha256": "LOWERCASE_SHA256_OF_FINAL_ARCHIVE",
  "size": 123456,
  "allow_downgrade": false
}
```

The version must match the release tag, archive basename and platform. Size is
in bytes, at most 1 GiB compressed; extraction is bounded to 4 GiB and 100,000
files. URLs require HTTPS, including redirects. Sign finalized archive metadata,
verify the signature with the configured public key, and read back the published
feed and archive before declaring the offer installable. Test fixtures in
`codeterm-update/src/headless/fixtures` use an ephemeral TEST key whose private
half was discarded; the release key rejects them.

### Periodic checks

The optional timer checks weekly without installing or restarting anything:

```bash
sudo install -m 0644 packaging/linux/domios-update-check.service /etc/systemd/system/
sudo install -m 0644 packaging/linux/domios-update-check.timer /etc/systemd/system/
sudo systemd-analyze verify /etc/systemd/system/domios-update-check.service /etc/systemd/system/domios-update-check.timer
sudo systemctl daemon-reload
sudo systemctl enable --now domios-update-check.timer
journalctl -u domios-update-check.service
```

The O2 drill installs the fixed helper, update unit and exact polkit rule on its
disposable VM. It checks the signed feed, then requests and verifies an available
update. Missing publication or an already-current signed version prints SKIP
with the reason. The release Team must validate signing/publication in release
CI; unit fixtures and a shipped drill are not evidence of a VM PASS.