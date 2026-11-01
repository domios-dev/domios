# Domios server in Docker

Production packaging consumes complete, signed headless Linux release archives.
The Dockerfile never compiles Domios. Both `linux/amd64` and `linux/arm64` are supported;
Docker's architecture maps to the release's `x86_64` and `aarch64` respectively.
CI qualifies a production-profile archive built from the candidate checkout
before a release is published, through a separate, explicitly enabled unsigned
smoke path. Image builds and runtime behavior must pass the Linux CI workflow
before publication. Local Windows image builds are not part of this procedure.

## Build from a verified release

On a Linux builder, obtain `domios-headless-vVERSION-linux-ARCH.tar.gz` and its
`domios-headless-linux-ARCH.json` envelope. The default verifier checks the
signed headless feed described in
[the Linux feed contract](../linux/README.md#feed-contract-for-the-release-team)
with the pinned public updater key in `release.pub`, then checks the signed
archive identity, byte size and SHA-256. CI verifies this public key matches
`src-tauri/tauri.conf.json`; update both when rotating the release key. A local
development build or checksum-only handoff cannot pass the production path.

Create a dedicated release context containing only each archive and its envelope.
No credentials, configuration, URLs or tokens belong in this context or in
build arguments. Inputs are read through a temporary BuildKit mount, so archives
and envelopes (including their download URLs) enter no image layer. The verifier
stage checks the signature, digest, release filename and
architecture, bounds extraction and refuses links and unsafe paths. Only the
verified resource tree reaches the runtime stage. The server also checks its
matching CLI and component manifest at startup. The digest-pinned Python slim
base supplies the standard-library health client and process supervisor; the
runtime installs no curl, root updater helper, polkit rules or systemd units.

```bash
docker buildx build --platform linux/amd64,linux/arm64 \
  --build-context release=/absolute/path/to/verified-release-files \
  --build-arg VERSION=VERSION \
  -f packaging/docker/Dockerfile -t domios-server:VERSION \
  --output type=oci,dest=domios-server.oci.tar .
```

For a single native platform, use `--platform linux/amd64 --load` instead of the
OCI output. Publication is a separate release-Team action; this repository adds
no registry push. The local example image name is not a promised registry name.

## Run and protect the data volume

The image sets `HOME=/var/lib/domios` for the `domios` service account.
All server state resides in `$HOME/.domios` (`/var/lib/domios/.domios`).
Configuration, memory, account credentials, Neighbor keys, secret
store, logs and model caches persist together. The image runs as UID/GID
`10001:10001`. New named volumes inherit the image directory's ownership and
mode `0700`; configuration is created with mode `0600` and umask `0077`.
The entrypoint refuses an existing data directory or config with another owner,
wrong mode, links or unexpected type; it never repairs permissions silently.
For a bind mount, provision the directory with `install -d -m 0700 -o 10001
-g 10001 /srv/domios-data` before starting it. Do not run the server as root.

```bash
docker volume create domios-data
docker run -d --name domios --restart unless-stopped --stop-timeout 30 \
  -p 127.0.0.1:7685:7685 \
  --mount source=domios-data,target=/var/lib/domios/.domios \
  domios-server:VERSION
docker inspect --format '{{.State.Health.Status}}' domios
```

The packaged initial config explicitly sets `api.host = "0.0.0.0"` **inside
the container network namespace** so Docker forwarding can reach it. This
setting is visible in `config.toml`; existing configuration is preserved.
It exposes authenticated API access to other containers on that Docker network.
Domios still requires its generated bearer token for protected API routes;
`/api/health` is the public liveness endpoint. The host publish is deliberately
loopback-only. Omitting `127.0.0.1` from `-p` exposes the API to host interfaces.
Use Mesh for remote access, with an explicitly provisioned Tailscale route and
same-account pairing; neither Tailscale nor provider CLIs ship in this image.
Never use host networking to work around container routing.

The initial config explicitly selects `neighbors.key_store = "file"` for
headless Linux. The T5 file store is unencrypted and opt-in: keys live on this
volume in `neighbor-keys/` (0700), with files 0600. The existing store refuses
foreign ownership, wide permissions, symlinks and hard links; startup also
validates any existing key directory and files. An available OS secret store
still takes precedence. See `domios docs NEIGHBORS`. Protect volume backups as
credentials. Do not mount a host secret-service socket into this container.

`HOME`, the Hugging Face cache and XDG cache point into this persistent layout.
Provision any desired embedding-model cache under `.domios/models` with matching
ownership. Initial local embedding can otherwise download its model at runtime;
no model, provider credential, token or deployment connection URL is baked in.
The supervisor starts the daemon directly through its protected-stdio protocol
in a new session and owns its pidfd before waiting for readiness. SIGTERM/SIGINT
handling starts before setup/spawn; queued stop requests reach that exact child
even while readiness is pending. Graceful cleanup is bounded to 20 seconds,
then a hung child is killed and reaped within two seconds. Keep Docker's stop
grace at 30 seconds (as in the run command and Compose). Docker's healthcheck
uses a bounded standard-library HTTP request to `/api/health` every five seconds.

The daemon writes its full log to `/var/lib/domios/.domios/logs/domios.log` on the
data volume. On an unexpected daemon exit or startup deadline, the supervisor
writes the failure and up to 80 lines / 8 KiB of the current startup's log to
container stderr, visible through `docker logs` or `docker compose logs domios`.
The excerpt redacts local secret values and credential URLs; it never prints
the protected readiness reply or its API token. If safe redaction is unavailable,
only an unavailable notice is printed. Inspect the protected volume log locally
for further detail; do not publish unredacted logs.

## Sign in without a browser in the container

```bash
docker exec -it domios domios account login --no-browser
```

Open the printed authorization link in your browser. After sign-in, copy the
**full callback URL**, including code and state, from the browser's failed
loopback redirect and paste it into the waiting CLI. A bare code is refused.
Keep the URL out of shell history, logs and messages. The existing PKCE,
state and single-use checks still apply. Check completion with
`docker exec domios domios account status`. Neighbor enrollment and any receiving
takeover require their separate explicit owner decision.

## Update by image tag

The Dockerfile creates `/opt/domios/.domios-container-managed`. The server and
CLI read this explicit installation marker; they do not inspect `/.dockerenv`,
cgroups or environment variables to guess container status. Both
`domios update check` and `domios update install` refuse with typed
`container_managed_update` data and instruct you to pull a newer image tag.
This also covers local-installer and automatic-install CLI options.
There is no privileged updater installation in the image.

Pull the release Team's published versioned image, stop/remove the old container,
then recreate it with that tag, the same data volume and the same loopback port
publication. For Compose, set `DOMIOS_IMAGE` to the new tag, then run
`docker compose -f packaging/docker/docker-compose.yml pull` and
`docker compose -f packaging/docker/docker-compose.yml up -d`. Recheck container
health. Retain a protected backup and the previous image until verified.

## Optional shared memory with pgvector

The Compose example uses `pgvector/pgvector:pg17`, a private database network,
a separate database volume and `depends_on: condition: service_healthy`.
Only Domios publishes a host port. Postgres reads its password from a Compose
secret file through `POSTGRES_PASSWORD_FILE`; no password is an environment
value. The Domios database URL belongs only in its active secret store as
`domios.mem:postgres_url`, never in config, environment, build args or the image.

Bootstrap the Domios volume with SQLite first, since the secret CLI requires a
running authenticated daemon. Even a fresh start creates bundled memory items.
After saving the URL, stop the server and explicitly migrate those items before
selecting Postgres. Selecting an empty Postgres database while SQLite has items
is deliberately refused; do not remove the SQLite store to bypass this guard.

```bash
umask 077
secret_dir=$(mktemp -d)
# Provision postgres_password and postgres_url in secret_dir from your secret manager.
# The URL uses host postgres, database/user domios and the same password,
# percent-encoded in the URL; keep both files private and out of Git.
export DOMIOS_IMAGE=domios-server:VERSION
export DOMIOS_POSTGRES_PASSWORD_FILE="$secret_dir/postgres_password"
# This environment value is a file path, never the password.
docker volume create domios-docker_domios-data
docker run -d --name domios-bootstrap \
  --mount source=domios-docker_domios-data,target=/var/lib/domios/.domios \
  "$DOMIOS_IMAGE"
# Wait for Docker's healthy state before the following command.
docker exec -i domios-bootstrap domios mem secret save --name domios.mem:postgres_url \
  < "$secret_dir/postgres_url"
docker stop domios-bootstrap
docker rm domios-bootstrap

# Start only the database; the server must remain stopped during migration.
docker compose -p domios-docker -f packaging/docker/docker-compose.yml \
  up -d --wait --wait-timeout 180 postgres
docker compose -p domios-docker -f packaging/docker/docker-compose.yml \
  run --rm domios migrate-memory --to postgres
# Check the migration JSON: verified and source_unchanged must both be true.
# Select the backend separately, only after the migration command succeeds.
docker compose -p domios-docker -f packaging/docker/docker-compose.yml \
  run --rm --no-deps domios select-memory --to postgres
docker compose -p domios-docker -f packaging/docker/docker-compose.yml \
  up -d --wait --wait-timeout 180
docker compose -p domios-docker -f packaging/docker/docker-compose.yml ps
docker compose -p domios-docker -f packaging/docker/docker-compose.yml logs domios
docker compose -p domios-docker -f packaging/docker/docker-compose.yml \
  exec postgres psql -U domios -d domios -c '\dt domios_mem.*'
```

Wait for Docker health to become healthy, then check `/api/health` reports
`startup.state = "ready"` on the chosen loopback host port. Use `domios mem search`
inside the running server container to verify migrated records. Keep the password
file private and available for subsequent Compose recreations; remove the URL
file after its secret-store save. Remove both temporary secret files when this
deployment is retired.

`migrate-memory` invokes the shipped server's existing offline migration and
returns its JSON and exit status. It starts no daemon and changes no config.
The local secret store works offline on the same data volume. Migration takes
the existing store ownership locks, retains SQLite, verifies all 21 tables and
reindexes when required. Remote embedding requires its configured consent and
credentials; the default local model may download into the persistent cache.
`select-memory --to postgres` changes only `memory.backend` in the private config
through an atomic write. Both one-off commands must run with the server stopped
and the same image, volume and configuration. They also support `--to sqlite`
for an explicitly verified reverse migration. See `domios docs MEMORY`.

The Compose command explicitly selects the `postgres` container configuration;
it refuses an existing configuration still selecting SQLite. All Domios
configuration stays on the same volume. Startup refuses a missing/empty secret,
a release without Postgres support or invalid database access. Database health
ordering does not replace the required credential provisioning.
For credential rotation, pipe the new URL into the existing container with
`docker exec -i domios domios mem secret save --name domios.mem:postgres_url --overwrite`,
update the database password separately, and recreate Domios. Changing the
Compose secret file alone does not change an initialized Postgres user's password.

## Backup

For SQLite memory/storage backups, mount a separate protected backup directory
and run `docker exec domios domios mem backup create --out /backups`.
The destination must be outside `.domios`. This command backs up memory and
storage, not account credentials, Neighbor keys, configuration or model caches.
Stop the container before taking a complete filesystem snapshot of `domios-data`;
preserve UID/GID and private modes when restoring. Never copy live SQLite/WAL
files as a substitute for the online backup command.

For Postgres, use protected `pg_dump`/`pg_restore` credentials and back up the
Domios data volume alongside the database. `domios mem backup create` is the
SQLite archive path; it does not back up Postgres. See `domios docs MEMORY`.

## CI smoke

The new `.github/workflows/docker-smoke.yml` runs on relevant pull requests or
manual dispatch. Each native Ubuntu amd64/arm64 job invokes the new named
operation `CI=true DOMIOS_DOCKER_SMOKE=1 CARGO_BUILD_TARGET=TARGET bash ./build.sh
docker-smoke-inputs`. It builds the frontend, CLI and server with `--release`,
checks source/CLI digest binding and the headless dependency closure, and uses
the shared headless packager to write the candidate archive under
`target/docker-smoke/archives/`. No release publication or signing secret is
needed. The existing repository already uses the native `ubuntu-24.04-arm`
runner; no additional runner type is required.

The Dockerfile's default `ARCHIVE_SOURCE=signed-release` refuses these unsigned
inputs. The smoke script alone selects `ARCHIVE_SOURCE=ci-smoke` **and**
`ALLOW_CI_SMOKE=1`, verifies its same-job archive SHA-256 and labels the result
`org.domios.archive-source=ci-smoke`. Either flag alone refuses the unsigned path.
These smoke images must never be published as production images.

Each job builds without a push, uses a temporary volume, consumes Docker health
events with a 180-second deadline, checks `/api/health`, proves typed check/install
refusals and graceful stop, then removes the container, volume, image and context.
No fixed sleep establishes readiness; the smoke publishes no host ports.

The exact Linux command is:

```bash
CI=true DOMIOS_DOCKER_SMOKE=1 bash scripts/docker/smoke.sh VERSION amd64 /path/to/domios-headless-vVERSION-linux-x86_64.tar.gz CI_ARCHIVE_SHA256
```

Use `arm64` and `linux-aarch64` for the other native job. A passing source check
does not prove the image; retain the workflow run as release evidence.
