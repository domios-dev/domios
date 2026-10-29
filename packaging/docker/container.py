import http.client
import json
import os
import pathlib
import selectors
import signal
import stat
import subprocess
import sys
import tomllib
import time

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


class ProcessRuntime:
    def __init__(self):
        self.child = None
        self.pidfd = None
        self.watcher = selectors.DefaultSelector()
        self.wake_read, self.wake_write = os.pipe2(os.O_NONBLOCK | os.O_CLOEXEC)
        self.watcher.register(self.wake_read, selectors.EVENT_READ, "signal")
        self.previous = {}
        self.reply = bytearray()

    def install(self, handler):
        for signum in (signal.SIGTERM, signal.SIGINT):
            self.previous[signum] = signal.signal(signum, handler)

    def wake(self):
        try:
            os.write(self.wake_write, b"s")
        except BlockingIOError:
            pass

    def start(self):
        self.child = subprocess.Popen(
            ["/opt/domios/domios-server/domios-server", "serve"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, start_new_session=True,
        )
        self.pidfd = os.pidfd_open(self.child.pid)
        self.watcher.register(self.pidfd, selectors.EVENT_READ, "exit")
        self.watcher.register(self.child.stdout, selectors.EVENT_READ, "ready")
        options = {"instance": {"kind": "production"}, "resources": "/opt/domios",
                   "probe_sink": None, "probe_identity_alias": None}
        self.child.stdin.write((json.dumps(options) + "\n").encode())
        self.child.stdin.close()

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
                    return 0 if self.requested is not None and not failed else 1
                if event == "ready" and not stopping:
                    deadline = None
                if event == "timeout":
                    if stopping:
                        self.runtime.forward(signal.SIGKILL)
                        self.runtime.reap(2)
                        return 1
                    failed = True
                    self.requested = signal.SIGTERM
        except BaseException:
            self.cleanup()
            raise
        finally:
            self.runtime.close()


def run(postgres=False):
    return Supervisor(ProcessRuntime()).run(lambda: prepare(postgres))


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
