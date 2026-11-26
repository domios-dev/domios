import http.client
import json
import os
import pathlib
try:
    import pwd
except ImportError:
    pwd = None
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
CLOUD_HOME = pathlib.Path("/var/lib/domios-cloud")
CLOUD_START = "/usr/local/bin/domios-cloud-start"
CLOUD_PROFILE = pathlib.Path("/etc/domios-cloud-profile")
CLOUD_USER = "node"


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


def prepare(postgres=False, data=None):
    data = DATA if data is None else pathlib.Path(data)
    os.umask(0o077)
    private(data, 0o700, directory=True)
    config = data / "config.toml"
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
    keys = data / "neighbor-keys"
    if keys.exists() or keys.is_symlink():
        private(keys, 0o700, directory=True)
        for key in keys.iterdir():
            private(key, 0o600)


class ProcessRuntime:
    def __init__(self, data=None, instance=None, pass_fds=()):
        self.data = DATA if data is None else pathlib.Path(data)
        self.instance = {"kind": "production"} if instance is None else instance
        self.pass_fds = tuple(pass_fds)
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
        log = self.data / "logs/domios.log"
        self.log_offset = log.stat().st_size if log.exists() else 0
        pass_fds = self.pass_fds
        self.pass_fds = ()
        try:
            self.child = subprocess.Popen(
                [SERVER, "serve"],
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, start_new_session=True,
                pass_fds=pass_fds,
            )
        finally:
            for descriptor in pass_fds:
                os.close(descriptor)
            if pass_fds:
                os.environ.pop("DOMIOS_CLOUD_BINDING_PIPE", None)
        self.pidfd = os.pidfd_open(self.child.pid)
        self.watcher.register(self.pidfd, selectors.EVENT_READ, "exit")
        self.watcher.register(self.child.stdout, selectors.EVENT_READ, "ready")
        options = {"instance": self.instance, "resources": "/opt/domios",
                   "probe_sink": None, "probe_identity_alias": None}
        self.child.stdin.write((json.dumps(options) + "\n").encode())
        self.child.stdin.close()

    def diagnostics(self, reason):
        print(f"Container daemon failed: {reason}", file=sys.stderr)
        if self.log_offset is None:
            return
        try:
            private(self.data / "logs", 0o700, directory=True)
            log = self.data / "logs/domios.log"
            private(log, 0o600)
            with log.open("rb") as stream:
                size = os.fstat(stream.fileno()).st_size
                offset = self.log_offset if size >= self.log_offset else 0
                stream.seek(max(offset, size - 65536))
                text = stream.read(65536).decode("utf-8", errors="replace")
            secrets_path = self.data / ".secrets"
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
        for descriptor in (self.pidfd, self.wake_read, self.wake_write, *self.pass_fds):
            if descriptor is not None:
                os.close(descriptor)
        self.pass_fds = ()
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


def drop_privileges(username=CLOUD_USER):
    if pwd is None:
        raise RuntimeError("POSIX privilege drop is unavailable")
    pw = pwd.getpwnam(username)
    os.initgroups(pw.pw_name, pw.pw_gid)
    os.setgid(pw.pw_gid)
    os.setuid(pw.pw_uid)
    if (os.getuid() != pw.pw_uid or os.geteuid() != pw.pw_uid
            or os.getgid() != pw.pw_gid or os.getegid() != pw.pw_gid):
        raise RuntimeError("Failed to drop root privileges")
    return pw


def stage_cloud_binding_pipe(env, *, strict=False):
    controller = env.pop("DOMIOS_BOOTSTRAP_CONTROLLER_KEY", None)
    nonce = env.pop("DOMIOS_BOOTSTRAP_NONCE", None)
    env.pop("DOMIOS_CLOUD_BINDING_PIPE", None)
    if controller is None and nonce is None:
        return None
    valid = all(
        isinstance(value, str) and re.fullmatch(r"[A-Za-z0-9_-]{43}", value)
        for value in (controller, nonce)
    )
    if not valid and strict:
        raise SystemExit("Invalid cloud creation binding")
    payload = (controller + "\n" + nonce + "\n").encode("ascii") if valid else b"invalid\n"
    reader, writer = os.pipe()
    with os.fdopen(writer, "wb") as stream:
        stream.write(payload)
    os.dup2(reader, 9, inheritable=True)
    os.set_inheritable(9, True)
    if reader != 9:
        os.close(reader)
    env["DOMIOS_CLOUD_BINDING_PIPE"] = "9"
    return 9


def run_fly_machines(args):
    if args:
        raise ValueError("Fly Machines entrypoint accepts no command arguments")
    if pwd is None:
        raise RuntimeError("Fly Machines entrypoint requires POSIX")
    info = CLOUD_HOME.lstat()
    if not stat.S_ISDIR(info.st_mode):
        raise RuntimeError(f"Refusing non-directory cloud volume mount: {CLOUD_HOME}")
    pw = pwd.getpwnam(CLOUD_USER)
    if os.geteuid() == 0:
        os.chown(CLOUD_HOME, pw.pw_uid, pw.pw_gid)
        os.chmod(CLOUD_HOME, 0o700)
        drop_privileges(CLOUD_USER)
    os.umask(0o077)
    private(CLOUD_HOME, 0o700, directory=True)
    os.environ["HOME"] = str(CLOUD_HOME)
    os.environ["USER"] = pw.pw_name
    os.environ["LOGNAME"] = pw.pw_name
    os.environ["XDG_CONFIG_HOME"] = str(CLOUD_HOME / ".config")
    os.environ["XDG_DATA_HOME"] = str(CLOUD_HOME / ".local/share")
    os.environ["XDG_CACHE_HOME"] = str(CLOUD_HOME / ".cache")
    os.environ["XDG_STATE_HOME"] = str(CLOUD_HOME / ".local/state")
    path_entries = [entry for entry in os.environ.get("PATH", "").split(":") if entry]
    if "/opt/domios/cli" not in path_entries:
        os.environ["PATH"] = ":".join(["/opt/domios/cli", *path_entries])
    os.chdir(CLOUD_HOME)
    profile = CLOUD_PROFILE.read_text().strip()
    data_name = {"qa": ".domios-dev", "release": ".domios"}.get(profile)
    if data_name is None:
        raise RuntimeError(f"Unknown cloud profile: {profile}")
    # Safe cold-wake / recoverable-recreate restore BEFORE starting daemon or publishing peer-ready.
    # Scrub ALL ephemeral restore/export secrets from env before execve so /proc/<pid>/environ never retains them.
    restore_archive = os.environ.pop("DOMIOS_RESTORE_ARCHIVE_PATH", None)
    restore_url = os.environ.pop("DOMIOS_RESTORE_ARCHIVE_URL", None)
    restore_sha256 = os.environ.pop("DOMIOS_RESTORE_ARCHIVE_SHA256", None)
    restore_key_hex = os.environ.pop("DOMIOS_RESTORE_ARCHIVE_KEY_HEX", None)
    restore_uid = os.environ.pop("DOMIOS_RESTORE_OWNER_UID", None)
    restore_machine = os.environ.pop("DOMIOS_RESTORE_MACHINE_ID", None)
    restore_gen_raw = os.environ.pop("DOMIOS_RESTORE_GENERATION", None)
    if restore_archive or restore_url:
        if not (restore_key_hex and restore_uid and restore_machine):
            raise RuntimeError("Incomplete cold-restore archive binding environment")
        import importlib.util as _ilu
        _arc_path = pathlib.Path("/usr/local/lib/domios-cloud-archive.py")
        if not _arc_path.exists():
            _arc_path = pathlib.Path(__file__).with_name("cloud_archive.py")
        _spec = _ilu.spec_from_file_location("domios_cloud_archive", _arc_path)
        _arc = _ilu.module_from_spec(_spec)
        sys.modules[_spec.name] = _arc
        _spec.loader.exec_module(_arc)
        _gen = int(restore_gen_raw) if restore_gen_raw else None
        _downloaded_tmp = None
        try:
            if restore_url and not restore_archive:
                _fd, _tmp_name = tempfile.mkstemp(prefix=".domios-restore-", suffix=".enc", dir=CLOUD_HOME)
                os.close(_fd)
                _downloaded_tmp = pathlib.Path(_tmp_name)
                os.chmod(_downloaded_tmp, 0o600)
                _arc.download_encrypted_archive(
                    restore_url,
                    _downloaded_tmp,
                    expected_sha256=restore_sha256 or None,
                )
                _src_archive = _downloaded_tmp
            else:
                _src_archive = pathlib.Path(restore_archive)
            _res = _arc.restore_essentials_archive(
                _src_archive,
                CLOUD_HOME,
                owner_key=bytes.fromhex(restore_key_hex),
                requester_uid=restore_uid,
                target_machine_id=restore_machine,
                expected_generation=_gen,
                expected_ciphertext_sha256=restore_sha256 or None,
            )
            if not _res.recovery_ready:
                raise RuntimeError(f"Cold restore incomplete: {_res.missing_or_partial}")
        finally:
            if _downloaded_tmp is not None:
                _downloaded_tmp.unlink(missing_ok=True)
    data_dir = CLOUD_HOME / data_name
    if not data_dir.exists() and not data_dir.is_symlink():
        os.mkdir(data_dir, 0o700)
    prepare(postgres=False, data=data_dir)
    # Stage fd 9 and scrub secrets before execve so /proc/<pid>/environ never retains them.
    stage_cloud_binding_pipe(os.environ, strict=False)
    os.execve(CLOUD_START, [CLOUD_START], dict(os.environ))


def command(args):
    platform = os.environ.get("DOMIOS_CLOUD_PLATFORM")
    if platform is not None:
        if platform != "fly_machines":
            raise ValueError(f"Unsupported DOMIOS_CLOUD_PLATFORM: {platform}")
        return run_fly_machines(args)
    if hasattr(os, "geteuid") and os.geteuid() == 0 and os.environ.get("DOMIOS_CLOUD_MACHINE") == "1":
        pw = drop_privileges(CLOUD_USER)
        os.environ.setdefault("HOME", pw.pw_dir)
        os.environ["USER"] = pw.pw_name
        os.environ["LOGNAME"] = pw.pw_name
        os.chdir(pw.pw_dir)
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
