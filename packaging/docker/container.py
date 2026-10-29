import http.client
import json
import os
import pathlib
import re
import selectors
import signal
import stat
import subprocess
import sys
import tempfile
import tomllib
import time
import urllib.parse

DATA = pathlib.Path.home() / ".domios"
SERVER = "/opt/domios/domios-server/domios-server"


def migrate_memory(backend):
    prepare()
    return subprocess.run([
        SERVER, "memory-migrate", "--to", backend, "--json",
    ], check=False).returncode


def select_memory(backend):
    prepare()
    path = DATA / "config.toml"
    text = path.read_text()
    expected = tomllib.loads(text)
    expected.setdefault("memory", {})["backend"] = backend
    section = re.search(r"(?ms)^\[memory\][ \t]*\n(.*?)(?=^\[|\Z)", text)
    setting = f'backend = "{backend}"\n'
    if section:
        body, count = re.subn(r"(?m)^[ \t]*backend[ \t]*=[^\n]*(?:\n|$)", setting, section[1])
        if count == 0:
            body = setting + section[1]
        text = text[:section.start(1)] + body + text[section.end(1):]
    else:
        text += "\n[memory]\n" + setting
    if tomllib.loads(text) != expected:
        raise RuntimeError("Refusing ambiguous memory configuration")
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", dir=DATA, prefix=".container-config-", delete=False) as output:
            temporary = pathlib.Path(output.name)
            os.chmod(temporary, 0o600)
            output.write(text)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)
    print(json.dumps({"memory.backend": backend}))
    return 0


def redact_diagnostics(text, secrets):
    values = []

    def collect(value, depth=0):
        if depth > 5:
            return
        if isinstance(value, dict):
            for item in value.values():
                collect(item, depth + 1)
        elif isinstance(value, str) and value:
            values.append(value)
            try:
                nested = json.loads(value)
            except ValueError:
                nested = None
            if isinstance(nested, dict):
                collect(nested, depth + 1)
            if "://" in value:
                password = urllib.parse.urlsplit(value).password
                if password:
                    values.extend([password, urllib.parse.unquote(password)])

    collect(secrets)
    for value in sorted(set(values), key=len, reverse=True):
        text = text.replace(value, "[REDACTED]")
    text = re.sub(r"(?i)\b(?:https?|postgres(?:ql)?)://\S+", "[REDACTED URL]", text)
    text = re.sub(r"(?i)\bbearer\s+\S+", "Bearer [REDACTED]", text)
    text = re.sub(r'(?i)([\"\w]*(?:token|password|secret|api_key)[\"\w]*\s*[:=]\s*)[^\s,}]+', r'\1[REDACTED]', text)
    return text


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


class ProcessRuntime:
    def __init__(self):
        self.child = None
        self.pidfd = None
        self.watcher = selectors.DefaultSelector()
        self.wake_read, self.wake_write = os.pipe2(os.O_NONBLOCK | os.O_CLOEXEC)
        self.watcher.register(self.wake_read, selectors.EVENT_READ, "signal")
        self.previous = {}
        self.reply = bytearray()
        self.log_offset = None

    def install(self, handler):
        for signum in (signal.SIGTERM, signal.SIGINT):
            self.previous[signum] = signal.signal(signum, handler)

    def wake(self):
        try:
            os.write(self.wake_write, b"s")
        except BlockingIOError:
            pass

    def start(self):
        log = DATA / "logs/domios.log"
        self.log_offset = log.stat().st_size if log.exists() else 0
        self.child = subprocess.Popen(
            [SERVER, "serve"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, start_new_session=True,
        )
        self.pidfd = os.pidfd_open(self.child.pid)
        self.watcher.register(self.pidfd, selectors.EVENT_READ, "exit")
        self.watcher.register(self.child.stdout, selectors.EVENT_READ, "ready")
        options = {"instance": {"kind": "production"}, "resources": "/opt/domios",
                   "probe_sink": None, "probe_identity_alias": None}
        self.child.stdin.write((json.dumps(options) + "\n").encode())
        self.child.stdin.close()

    def diagnostics(self, reason):
        print(f"Container daemon failed: {reason}", file=sys.stderr)
        if self.log_offset is None:
            return
        try:
            private(DATA / "logs", 0o700, directory=True)
            log = DATA / "logs/domios.log"
            private(log, 0o600)
            with log.open("rb") as stream:
                size = os.fstat(stream.fileno()).st_size
                offset = self.log_offset if size >= self.log_offset else 0
                stream.seek(max(offset, size - 65536))
                text = stream.read(65536).decode("utf-8", errors="replace")
            secrets_path = DATA / ".secrets"
            secrets = {}
            if secrets_path.exists():
                private(secrets_path, 0o600)
                with secrets_path.open("rb") as stream:
                    secrets = json.loads(stream.read(1048577))
            text = redact_diagnostics(text, secrets)
            lines = text.splitlines()[-80:]
            tail = "\n".join(lines).encode("utf-8")[-8192:].decode("utf-8", errors="ignore")
            print("Daemon log tail (bounded and redacted):\n" + tail, file=sys.stderr)
        except (OSError, ValueError, RuntimeError):
            print("Daemon log tail unavailable; inspect the protected data-volume logs", file=sys.stderr)

    def forward(self, signum):
        try:
            if self.pidfd is not None:
                signal.pidfd_send_signal(self.pidfd, signum)
            elif self.child is not None:
                self.child.send_signal(signum)
        except ProcessLookupError:
            pass

    def wait(self, deadline):
        timeout = None if deadline is None else max(0, deadline - time.monotonic())
        events = self.watcher.select(timeout)
        if not events:
            return "timeout"
        kinds = {key.data for key, _ in events}
        if "exit" in kinds:
            return "exit"
        if "signal" in kinds:
            os.read(self.wake_read, 4096)
            return "signal"
        chunk = os.read(self.child.stdout.fileno(), 65536)
        self.reply.extend(chunk)
        if len(self.reply) > 65536 or not chunk:
            raise RuntimeError("Daemon readiness pipe refused")
        if b"\n" not in self.reply:
            return "pending"
        try:
            reply = json.loads(self.reply)
            if reply["ready"]["process"]["pid"] != self.child.pid:
                raise ValueError("Wrong daemon identity")
        except (KeyError, TypeError, ValueError) as error:
            raise RuntimeError("Daemon readiness identity refused") from error
        finally:
            self.reply.clear()
        self.watcher.unregister(self.child.stdout)
        self.child.stdout.close()
        return "ready"

    def reap(self, timeout):
        if self.child is not None:
            self.child.wait(timeout=timeout)

    def close(self):
        for signum, previous in self.previous.items():
            signal.signal(signum, previous)
        self.watcher.close()
        for descriptor in (self.pidfd, self.wake_read, self.wake_write):
            if descriptor is not None:
                os.close(descriptor)
        if self.child is not None:
            self.child.stdout.close()
            self.child.stdin.close()


class Supervisor:
    def __init__(self, runtime, clock=time.monotonic):
        self.runtime = runtime
        self.clock = clock
        self.requested = None

    def stop(self, signum, frame=None):
        if self.requested is None:
            self.requested = signum
        self.runtime.wake()

    def cleanup(self):
        self.runtime.forward(signal.SIGTERM)
        try:
            self.runtime.reap(20)
        except subprocess.TimeoutExpired:
            self.runtime.forward(signal.SIGKILL)
            self.runtime.reap(2)

    def run(self, prepare_data):
        self.runtime.install(self.stop)
        try:
            prepare_data()
            if self.requested is not None:
                return 0
            self.runtime.start()
            deadline = self.clock() + 120
            stopping = False
            failed = False
            while True:
                if self.requested is not None and not stopping:
                    stopping = True
                    self.runtime.forward(self.requested)
                    deadline = self.clock() + 20
                event = self.runtime.wait(deadline)
                if event == "exit":
                    self.runtime.reap(0)
                    if self.requested is None or failed:
                        self.runtime.diagnostics("unexpected exit")
                    return 0 if self.requested is not None and not failed else 1
                if event == "ready" and not stopping:
                    deadline = None
                if event == "timeout":
                    if stopping:
                        self.runtime.forward(signal.SIGKILL)
                        self.runtime.reap(2)
                        self.runtime.diagnostics("stop deadline exceeded")
                        return 1
                    failed = True
                    self.requested = signal.SIGTERM
        except BaseException:
            self.cleanup()
            self.runtime.diagnostics("startup supervision failed")
            raise
        finally:
            self.runtime.close()


def run(postgres=False):
    return Supervisor(ProcessRuntime()).run(lambda: prepare(postgres))


def command(args):
    if args == ["health"]:
        health()
        return 0
    if args in ([], ["postgres"]):
        return run(args == ["postgres"])
    if (len(args) == 3 and args[0] in ("migrate-memory", "select-memory")
            and args[1] == "--to" and args[2] in ("sqlite", "postgres")):
        return migrate_memory(args[2]) if args[0] == "migrate-memory" else select_memory(args[2])
    raise ValueError("Expected postgres, health, migrate-memory --to BACKEND, or select-memory --to BACKEND")


if __name__ == "__main__":
    try:
        sys.exit(command(sys.argv[1:]))
    except Exception as error:
        print(f"Container refused: {error}", file=sys.stderr)
        sys.exit(1)
