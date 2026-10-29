import http.client
import json
import os
import pathlib
import select
import signal
import stat
import subprocess
import sys
import tomllib

DATA = pathlib.Path.home() / ".domios"


def health():
    connection = http.client.HTTPConnection("127.0.0.1", 7685, timeout=3)
    try:
        connection.request("GET", "/api/health")
        response = connection.getresponse()
        body = json.loads(response.read(65536))
        if (response.status != 200 or not isinstance(body, dict)
                or body.get("startup", {}).get("state") != "ready"):
            raise RuntimeError("Server health refused")
    finally:
        connection.close()


def private(path, mode, directory=False):
    info = path.lstat()
    expected = stat.S_ISDIR if directory else stat.S_ISREG
    if (not expected(info.st_mode) or info.st_uid != os.getuid()
            or stat.S_IMODE(info.st_mode) != mode
            or (not directory and info.st_nlink != 1)):
        raise RuntimeError(f"Refusing unsafe owner, type, links or mode: {path}")


def prepare(postgres=False):
    os.umask(0o077)
    private(DATA, 0o700, directory=True)
    config = DATA / "config.toml"
    if not config.exists() and not config.is_symlink():
        with config.open("xb") as output:
            template = "domios-container-postgres.toml" if postgres else "domios-container.toml"
            output.write((pathlib.Path("/usr/local/share") / template).read_bytes())
    private(config, 0o600)
    with config.open("rb") as stream:
        settings = tomllib.load(stream)
    if settings.get("neighbors", {}).get("key_store") != "file":
        raise RuntimeError('Container requires explicit neighbors.key_store = "file"')
    if postgres and settings.get("memory", {}).get("backend") != "postgres":
        raise RuntimeError('Compose requires memory.backend = "postgres"; finish bootstrap first')
    keys = DATA / "neighbor-keys"
    if keys.exists() or keys.is_symlink():
        private(keys, 0o700, directory=True)
        for key in keys.iterdir():
            private(key, 0o600)


def run(postgres=False):
    prepare(postgres)
    ready = subprocess.run(
        ["/opt/domios/domios-server/domios-server", "start", "--production",
         "--resources", "/opt/domios", "--json"],
        check=True, stdout=subprocess.PIPE, timeout=120,
    )
    process = json.loads(ready.stdout)["process"]
    pidfd = os.pidfd_open(process["pid"])
    stopping = False

    def stop(signum, frame):
        nonlocal stopping
        stopping = True
        try:
            signal.pidfd_send_signal(pidfd, signum)
        except ProcessLookupError:
            pass

    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    watcher = select.poll()
    watcher.register(pidfd, select.POLLIN)
    watcher.poll()
    os.close(pidfd)
    while True:
        try:
            reaped, _ = os.waitpid(-1, os.WNOHANG)
        except ChildProcessError:
            break
        else:
            if reaped == 0:
                break
    return 0 if stopping else 1


if __name__ == "__main__":
    try:
        if sys.argv[1:] == ["health"]:
            health()
        elif sys.argv[1:] in ([], ["postgres"]):
            sys.exit(run(sys.argv[1:] == ["postgres"]))
        else:
            raise ValueError("Expected no arguments, or health")
    except Exception as error:
        print(f"Container refused: {error}", file=sys.stderr)
        sys.exit(1)
