#!/usr/bin/env bash
set -euo pipefail
if [[ $# != 4 ]]; then
  echo 'Usage: bash scripts/docker/smoke.sh VERSION amd64|arm64 ARCHIVE TRUSTED_SHA256' >&2
  exit 2
fi
version=$1
architecture=$2
archive=$3
checksum=$4
[[ $(uname -s) == Linux ]] || { echo 'Linux CI only' >&2; exit 2; }
[[ ${CI:-} == true && ${DOMIOS_DOCKER_SMOKE:-} == 1 ]] || { echo 'Explicit CI smoke mode required' >&2; exit 2; }
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ && $checksum =~ ^[a-f0-9]{64}$ ]] || exit 2
case "$architecture" in
  amd64) release_arch=x86_64 ;;
  arm64) release_arch=aarch64 ;;
  *) exit 2 ;;
esac
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
temporary=$(mktemp -d)
name="domios-smoke-$(basename "$temporary" | tr '[:upper:]' '[:lower:]')"
image="$name:$version"
cleanup() {
  docker rm -f "$name" >/dev/null 2>&1 || true
  docker volume rm "$name" >/dev/null 2>&1 || true
  docker image rm "$image" >/dev/null 2>&1 || true
  rm -rf "$temporary"
}
trap cleanup EXIT
filename="domios-headless-v$version-linux-$release_arch.tar.gz"
cp "$archive" "$temporary/$filename"
printf '%s  %s\n' "$checksum" "$filename" > "$temporary/$filename.sha256"
docker buildx build --load --platform "linux/$architecture" \
  --build-context "release=$temporary" --build-arg "VERSION=$version" \
  --build-arg ARCHIVE_SOURCE=ci-smoke --build-arg ALLOW_CI_SMOKE=1 \
  -f "$root/packaging/docker/Dockerfile" -t "$image" "$root"
docker volume create "$name" >/dev/null
docker create --name "$name" --mount "source=$name,target=/var/lib/domios/.domios" "$image" >/dev/null
# Subscribe before start; --since retains health transitions if subscription admission races.
python3 - "$name" <<'PY'
import datetime
import json
import selectors
import subprocess
import sys
import time

name = sys.argv[1]
since = datetime.datetime.now(datetime.timezone.utc).isoformat()
events = subprocess.Popen([
    "docker", "events", "--since", since, "--filter", f"container={name}",
    "--format", "{{json .}}",
], stdout=subprocess.PIPE, bufsize=0)
try:
    subprocess.run(["docker", "start", name], check=True, stdout=subprocess.DEVNULL)
    watcher = selectors.DefaultSelector()
    watcher.register(events.stdout, selectors.EVENT_READ)
    deadline = time.monotonic() + 180
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0 or not watcher.select(remaining):
            raise RuntimeError("Container did not become healthy before deadline")
        line = events.stdout.readline()
        if not line:
            raise RuntimeError("Docker event stream ended")
        action = json.loads(line).get("Action", "")
        if action == "health_status: healthy":
            state = json.loads(subprocess.check_output(["docker", "inspect", name]))[0]["State"]
            if state["Running"] and state["Health"]["Status"] == "healthy":
                break
        if action in ("die", "health_status: unhealthy"):
            raise RuntimeError("Container failed startup or health")
finally:
    events.terminate()
    events.wait(timeout=10)
PY
docker exec "$name" python /usr/local/lib/domios-container.py health
for action in check install; do
  set +e
  docker exec "$name" domios --json update "$action" > "$temporary/refusal.json" 2> "$temporary/refusal.stderr"
  result=$?
  set -e
  [[ $result == 1 ]] || { echo "update $action exit=$result; expected 1" >&2; exit 1; }
  python3 - "$temporary/refusal.json" <<'PY'
import json
import pathlib
import sys
body = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert body["ok"] is False
assert "container_managed_update" in json.dumps(body)
assert "Pull a newer image tag" in body["error"]["message"]
PY
done
docker stop --time 30 "$name" >/dev/null
[[ $(docker inspect --format '{{.State.ExitCode}}' "$name") == 0 ]]
echo "PASS: linux/$architecture health, typed check/install refusal, graceful stop"
