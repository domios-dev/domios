import json
import os
import queue
import re
import subprocess
import sys
import threading
import time


url = sys.argv[1]
pattern = re.compile(sys.argv[2], re.DOTALL)
timeout = float(sys.argv[3])
token = os.environ["CODETERM_API_TOKEN"]
lines: queue.Queue[bytes | None] = queue.Queue()
process = subprocess.Popen(
    [
        "curl",
        "--no-buffer",
        "--silent",
        "--show-error",
        "--max-time",
        str(timeout),
        "-H",
        f"Authorization: Bearer {token}",
        url,
    ],
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
)


def read_lines() -> None:
    assert process.stdout is not None
    for line in iter(process.stdout.readline, b""):
        lines.put(line)
    lines.put(None)


threading.Thread(target=read_lines, daemon=True).start()
deadline = time.monotonic() + timeout
observed = ""
try:
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise queue.Empty
        line = lines.get(timeout=remaining)
        if line is None:
            raise SystemExit("output stream ended before the event")
        if not line.startswith(b"data:"):
            continue
        frame = json.loads(line.split(b":", 1)[1])
        observed = (observed + frame.get("output", ""))[-2_000_000:]
        match = pattern.search(observed)
        if match:
            print(match.group(0).encode("unicode_escape").decode("ascii"))
            break
except queue.Empty:
    raise SystemExit(f"timed out after {timeout:g}s waiting for PTY output: {pattern.pattern}")
finally:
    process.terminate()
    try:
        process.wait(timeout=2)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()
