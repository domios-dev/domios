"""WHAT: Invoke dc and observe the named instance's event stream.
WHY: Inherited production identity and timer-based readiness are unsafe.
ORACLE: CLI responses, ordered SSE frames and exact instance identity agree.
"""

import json
import os
from pathlib import Path
import queue
import signal
import socket
import subprocess
import threading
import time
import urllib.request

from fixtures import REPO


class Finding(RuntimeError):
    pass


class Pending(RuntimeError):
    pass


def read_secret():
    import sys
    value = []
    done = threading.Event()
    def read():
        value.append(sys.stdin.buffer.read(65537))
        done.set()
    threading.Thread(target=read, daemon=True).start()
    if not done.wait(10):
        raise Finding("Token stdin did not complete within its failure deadline")
    if not value or not 0 < len(value[0]) <= 65536:
        raise Finding("Token stdin is empty or oversized")
    return value[0]


class InstanceLock:
    def __init__(self, root):
        self.path = root / "run.lock"

    def __enter__(self):
        self.file = self.path.open("a+b")
        try:
            if os.name == "nt":
                import msvcrt
                self.file.write(b"0")
                self.file.flush()
                self.file.seek(0)
                msvcrt.locking(self.file.fileno(), msvcrt.LK_NBLCK, 1)
            else:
                import fcntl
                fcntl.flock(self.file.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError as error:
            self.file.close()
            raise Finding("Another gate owns this named instance; no retry") from error
        return self

    def __exit__(self, *_):
        self.file.close()


def json_value(text):
    decoder = json.JSONDecoder()
    for index, char in enumerate(text):
        if char not in "[{":
            continue
        try:
            value, length = decoder.raw_decode(text[index:])
        except ValueError:
            continue
        if not text[index + length:].strip():
            return value
    raise Finding("CLI did not return a complete JSON document")


def run_group(argv, env, timeout, stdin=None, *, cwd=None):
    process = subprocess.Popen(
        argv, cwd=REPO if cwd is None else cwd, env=env, start_new_session=True,
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    )
    try:
        out, err = process.communicate(stdin, timeout=timeout)
    except BaseException:
        if os.name == "nt":
            subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5)
        else:
            os.killpg(process.pid, signal.SIGKILL)
        process.communicate(timeout=5)
        raise
    return process.returncode, out.decode(errors="replace"), err.decode(errors="replace")


class Transcript:
    def __init__(self, directory):
        self.directory = Path(directory)
        self.directory.mkdir(parents=True, exist_ok=True)
        self.lock = threading.Lock()
        self.sequence = 0
        self.bytes = 0

    def record(self, kind, value):
        with self.lock:
            self.sequence += 1
            line = json.dumps({"n": self.sequence, "kind": kind, "value": value}) + "\n"
            self.bytes += len(line.encode())
            if self.bytes > 16 * 1024 * 1024:
                raise Finding("Transcript exceeded its 16 MiB cap")
            with (self.directory / "transcript.jsonl").open("a") as output:
                output.write(line)
            return self.sequence


class Dev:
    def __init__(self, env, transcript, cap=180):
        self.env = env
        self.transcript = transcript
        self.deadline = time.monotonic() + cap

    def remaining(self):
        remaining = self.deadline - time.monotonic()
        if remaining <= 0:
            raise Finding("Global case deadline expired")
        return remaining

    def dc(self, *args, stdin=None, json_mode=True, allow_error=False):
        command = ["bash", "-c", 'source "$1"; shift; dc "$@"', "matrix-dc",
                   str(REPO / "evals/_lib/lib.sh"), *map(str, args)]
        if json_mode:
            command.append("--json")
        self.transcript.record("command", {"dc": list(args), "json": json_mode})
        code, out, err = run_group(command, self.env, min(35, self.remaining()), stdin)
        self.transcript.record("response", {"exit": code, "stdout": out, "stderr": err})
        if code and not allow_error:
            raise Finding(f"dc {' '.join(map(str, args))} exited {code}: {err or out}")
        if json_mode and not code:
            return json_value(out)
        return out

    def snapshot(self, tab):
        payload = self.dc("tab", "inspect", "--tab", tab, "--lifecycle")
        candidates = objects(payload)
        match = next((item for item in candidates if item.get("id") == tab), None)
        if match is None:
            raise Finding(f"Full tab payload absent for {tab}")
        self.transcript.record("snapshot", payload)
        screen = self.dc("tab", "get-text", "--tab", tab)
        self.transcript.record("screen", {"tab_id": tab, "screen": screen})
        return match, payload

    def get(self, path):
        if path != "/api/supporter/activity?limit=100":
            raise Finding("Direct API read is outside the bound CLI-less activity surface")
        request = urllib.request.Request(
            f"http://127.0.0.1:{self.env['CODETERM_API_PORT']}{path}",
            headers={"Authorization": "Bearer " + self.env["CODETERM_API_TOKEN"]},
        )
        self.transcript.record("api_read", path)
        with urllib.request.urlopen(request, timeout=min(5, self.remaining())) as response:
            payload = response.read(256 * 1024 + 1)
        if len(payload) > 256 * 1024:
            raise Finding("Activity API payload exceeds read cap")
        result = json.loads(payload)
        self.transcript.record("api_response", result)
        return result


def objects(value):
    if isinstance(value, dict):
        yield value
        for child in value.values():
            yield from objects(child)
    elif isinstance(value, list):
        for child in value:
            yield from objects(child)


class Events:
    def __init__(self, dev):
        self.dev = dev
        self.frames = queue.Queue(maxsize=4096)
        self.ready = threading.Event()
        self.error = None
        self.response = None
        self.thread = threading.Thread(target=self.read, daemon=True)

    def read(self):
        try:
            env = self.dev.env
            request = urllib.request.Request(
                f"http://127.0.0.1:{env['CODETERM_API_PORT']}/api/events/stream",
                headers={"Authorization": "Bearer " + env["CODETERM_API_TOKEN"]},
            )
            self.response = urllib.request.urlopen(request, timeout=self.dev.remaining())
            self.ready.set()
            data = []
            while True:
                raw = self.response.readline(1024 * 1024)
                if not raw:
                    raise Finding("SSE stream closed before case completion")
                line = raw.decode().strip()
                if line.startswith("data:"):
                    data.append(line[5:].strip())
                elif not line and data:
                    frame = json.loads("\n".join(data))
                    data.clear()
                    if frame.get("event_type") == "overflow":
                        raise Finding("SSE overflow invalidates negative and order evidence")
                    self.dev.transcript.record("event", frame)
                    self.frames.put_nowait(frame)
        except BaseException as error:
            self.error = error
            self.ready.set()
            try:
                self.frames.put_nowait({"error": str(error)})
            except queue.Full:
                pass

    def __enter__(self):
        self.thread.start()
        if not self.ready.wait(min(10, self.dev.remaining())) or self.error:
            raise Finding(f"Event subscription did not acknowledge: {self.error}")
        return self

    def next(self):
        try:
            frame = self.frames.get(timeout=self.dev.remaining())
        except queue.Empty as error:
            raise Finding("Observable event deadline expired") from error
        if "error" in frame or self.error:
            raise Finding(f"Event evidence unavailable: {self.error or frame}")
        return frame

    def until(self, condition, describe):
        while True:
            value = condition()
            if value:
                return value
            self.dev.transcript.record("await", describe)
            self.next()

    def __exit__(self, *_):
        if self.response:
            try:
                self.response.fp.raw._sock.shutdown(socket.SHUT_RDWR)
            except (AttributeError, OSError):
                pass
            self.response.close()
        self.thread.join(timeout=2)
