#!/usr/bin/env bash
set -Eeuo pipefail
umask 077

step=preconditions
scratch=
tunnel_open=false
cleanup() {
  if "$tunnel_open"; then
    ssh -S "$scratch/ssh-control" -O exit "$DOMIOS_DRILL_SSH_TARGET" >/dev/null 2>&1 || true
  fi
  unset token
}
trap cleanup EXIT
trap 'code=$?; printf "FAIL %s (exit %s)\n" "$step" "$code" >&2; exit "$code"' ERR
begin() { step=$1; }
pass() { printf 'PASS %s\n' "$step"; }
refuse() { printf 'FAIL %s: %s\n' "$step" "$1" >&2; exit 1; }

[[ $(uname -s) == Linux && $EUID == 0 ]] || refuse 'run as root on the owner-provided clean Linux VM'
[[ ${DOMIOS_DRILL_CLEAN_VM:-} == 1 ]] || refuse 'set DOMIOS_DRILL_CLEAN_VM=1 only on a disposable clean VM'
[[ -d /run/systemd/system ]] || refuse 'systemd is not running'
[[ -z ${DISPLAY:-} && -z ${WAYLAND_DISPLAY:-} ]] || refuse 'a desktop session is present'
for command in systemctl python3 curl runuser useradd install cmp; do
  command -v "$command" >/dev/null || refuse "missing command: $command"
done
if id domios-server >/dev/null 2>&1; then refuse 'domios-server user already exists; refusing existing data'; fi
[[ ! -e /var/lib/domios-server && ! -e /etc/systemd/system/domios-server.service ]] || refuse 'existing installation found'
bundle=/opt/domios
[[ -x $bundle/domios-server/domios-server && -x $bundle/cli/codeterm && -f $bundle/dist/index.html ]] || refuse 'install the complete verified release bundle at /opt/domios first'
[[ -n ${DOMIOS_MODEL_CACHE:-} && -d $DOMIOS_MODEL_CACHE ]] || refuse 'set DOMIOS_MODEL_CACHE to the prepared offline Hugging Face model cache'
python3 - <<'PY'
import socket
with socket.socket() as listener:
    listener.bind(('127.0.0.1', 7685))
PY
repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
scratch=$(mktemp -d /var/tmp/domios-linux-drill.XXXXXXXX)
printf 'Drill evidence: %s\n' "$scratch"
pass

begin install-unit
useradd --system --user-group --create-home --home-dir /var/lib/domios-server --shell /bin/bash domios-server
install -d -m 0700 -o domios-server -g domios-server /var/lib/domios-server/.domios /var/lib/domios-server/backups
cp -a -- "$DOMIOS_MODEL_CACHE" /var/lib/domios-server/models
chown -R domios-server:domios-server /var/lib/domios-server/models
cat > "$scratch/config.toml" <<'TOML'
[api]
host = "127.0.0.1"
port = 7685

[neighbors]
enabled = true
key_store = "file"

[memory.embedder]
provider = "local"
remote_opt_in = false
TOML
install -m 0600 -o domios-server -g domios-server "$scratch/config.toml" /var/lib/domios-server/.domios/config.toml
install -m 0644 "$repo/packaging/linux/domios-server.service" /etc/systemd/system/domios-server.service
systemctl daemon-reload
pass

begin verify-unit
if command -v systemd-analyze >/dev/null; then
  systemd-analyze verify /etc/systemd/system/domios-server.service
  pass
else
  printf 'SKIP %s: systemd-analyze is unavailable\n' "$step"
fi

as_user() {
  runuser -u domios-server -- env -i HOME=/var/lib/domios-server USER=domios-server LOGNAME=domios-server \
    PATH=/opt/domios/cli:/usr/local/bin:/usr/bin:/bin HF_HOME=/var/lib/domios-server/models HF_HUB_OFFLINE=1 \
    /opt/domios/cli/codeterm "$@"
}

begin start-and-health
systemctl start domios-server
systemctl is-active --quiet domios-server
curl --fail --silent --show-error --max-time 10 http://127.0.0.1:7685/api/health > "$scratch/health.json"
python3 - "$scratch/health.json" <<'PY'
import json, sys
health = json.load(open(sys.argv[1]))
assert health['startup']['state'] == 'ready', health
assert health.get('build_sha'), health
PY
as_user identify > "$scratch/identity.txt"
pass

begin web-entry
curl --fail --silent --show-error --max-time 10 http://127.0.0.1:7685/ > "$scratch/index.html"
python3 - "$scratch/index.html" <<'PY'
import sys
page = open(sys.argv[1]).read().lower()
assert '<html' in page and '<script' in page, 'frontend entry page is missing'
PY
pass

begin ssh-tunnel-web-and-health
if [[ -z ${DOMIOS_DRILL_SSH_TARGET:-} ]]; then
  printf 'SKIP %s: no real SSH fixture supplied (set DOMIOS_DRILL_SSH_TARGET to this VM via its test SSH account)\n' "$step"
else
  command -v ssh >/dev/null
  ssh -M -S "$scratch/ssh-control" -fN -o BatchMode=yes -o ExitOnForwardFailure=yes -o ConnectTimeout=10 \
    -L 127.0.0.1:17685:127.0.0.1:7685 "$DOMIOS_DRILL_SSH_TARGET"
  tunnel_open=true
  curl --fail --silent --show-error --max-time 10 http://127.0.0.1:17685/api/health > "$scratch/tunnel-health.json"
  curl --fail --silent --show-error --max-time 10 http://127.0.0.1:17685/ > "$scratch/tunnel-index.html"
  cmp "$scratch/index.html" "$scratch/tunnel-index.html"
  python3 - "$scratch/health.json" "$scratch/tunnel-health.json" <<'PY'
import json, sys
direct, tunnel = (json.load(open(p)) for p in sys.argv[1:])
assert direct['boot_id'] == tunnel['boot_id'] and direct['build_sha'] == tunnel['build_sha']
assert tunnel['startup']['state'] == 'ready'
PY
  token=$(as_user system api-token)
  printf 'header = "Authorization: Bearer %s"\n' "$token" | \
    curl --config - --fail --silent --show-error --max-time 10 http://127.0.0.1:17685/api/account/status > "$scratch/tunnel-status.json"
  unset token
  ssh -S "$scratch/ssh-control" -O exit "$DOMIOS_DRILL_SSH_TARGET"
  tunnel_open=false
  pass
fi

begin browserless-sign-in
printf 'Use the dedicated paid test account. Paste the whole redirected URL when prompted.\n'
as_user account login --no-browser
as_user --json account status > "$scratch/account-status.json"
python3 - "$scratch/account-status.json" <<'PY'
import json, sys
status = json.load(open(sys.argv[1]))
assert status['identity'] and status['provider'] != 'local', status
PY
pass

begin file-key-enrollment
as_user --json account neighbors status > "$scratch/neighbors-before.json"
revision=$(python3 - "$scratch/neighbors-before.json" <<'PY'
import json, sys
print(json.load(open(sys.argv[1]))['revision'])
PY
)
as_user account neighbors enroll --confirm-takeover --expected-receiver-revision "$revision"
as_user account neighbors own-keys > "$scratch/own-keys.json"
python3 - <<'PY'
import os, pwd, stat
root = '/var/lib/domios-server/.domios/neighbor-keys'
uid = pwd.getpwnam('domios-server').pw_uid
directory = os.lstat(root)
assert stat.S_ISDIR(directory.st_mode) and stat.S_IMODE(directory.st_mode) == 0o700 and directory.st_uid == uid
paths = [os.path.join(root, name) for name in os.listdir(root)]
assert paths, 'no file keys were enrolled'
for path in paths:
    metadata = os.lstat(path)
    assert stat.S_ISREG(metadata.st_mode) and stat.S_IMODE(metadata.st_mode) == 0o600
    assert metadata.st_uid == uid and metadata.st_nlink == 1 and metadata.st_size == 64
PY
pass

begin online-backup
as_user --json mem save --kind note --area headless-drill --title headless-drill \
  'Disposable Linux VM drill memory: backup and restore must retain this record.' > "$scratch/saved.json"
item_id=$(python3 - "$scratch/saved.json" <<'PY'
import json, sys
value = json.load(open(sys.argv[1]))
print(value['id'])
PY
)
install -d -m 0700 -o domios-server -g domios-server /var/lib/domios-server/.domios/storage
printf 'headless-drill-storage\n' > "$scratch/expected-storage"
install -m 0600 -o domios-server -g domios-server "$scratch/expected-storage" /var/lib/domios-server/.domios/storage/drill-proof.bin
as_user --json mem backup create --out /var/lib/domios-server/backups > "$scratch/backup.json"
archive=$(python3 - "$scratch/backup.json" <<'PY'
import json, sys
print(json.load(open(sys.argv[1]))['backup'])
PY
)
[[ -f $archive/manifest.json && -f $archive/mem/knowledge.db && -d $archive/storage ]]
pass

begin refuse-live-restore
if as_user mem backup restore --from "$archive" > "$scratch/live-restore.txt" 2>&1; then
  refuse 'restore succeeded while the daemon was running'
fi
python3 - "$scratch/live-restore.txt" <<'PY'
import sys
assert 'stopped daemon' in open(sys.argv[1]).read(), 'restore failed for an unrelated reason'
PY
pass

begin stopped-restore
systemctl stop domios-server
if systemctl is-active --quiet domios-server; then refuse 'systemd did not stop the server'; fi
printf 'changed\n' > /var/lib/domios-server/.domios/storage/drill-proof.bin
as_user --json mem backup restore --from "$archive" > "$scratch/restore.json"
cmp "$scratch/expected-storage" /var/lib/domios-server/.domios/storage/drill-proof.bin
cmp "$archive/mem/knowledge.db" /var/lib/domios-server/.domios/mem/knowledge.db
pass

begin restart-and-read
systemctl start domios-server
as_user --json mem get "$item_id" > "$scratch/restored-item.json"
python3 - "$scratch/restored-item.json" <<'PY'
import json, sys
item = json.load(open(sys.argv[1]))
assert 'Disposable Linux VM drill memory' in json.dumps(item)
PY
pass

begin update-discovery
as_user --json update check --force > "$scratch/update-check.json"
python3 - "$scratch/update-check.json" <<'PY'
import json, sys
result = json.load(open(sys.argv[1]))
assert not result.get('errors', {}).get('app'), result
assert any(item['id'] == 'app' for item in result['items']), result
PY
printf 'PASS %s (existing desktop application feed; not a signed server update offer)\n' "$step"
printf 'SKIP update-install: verified headless install is unsupported; headless archives have no signed update feed or systemd installer (COD-2005)\n'
printf 'PASS required Linux drill steps; review the explicit SKIP rows. Evidence: %s\n' "$scratch"
