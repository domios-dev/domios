import queue
import re
import subprocess
import sys
import threading
import time


path = sys.argv[1]
offset = int(sys.argv[2])
pattern = re.compile(sys.argv[3])
timeout = float(sys.argv[4])
required_matches = int(sys.argv[5]) if len(sys.argv) > 5 else 1
lines: queue.Queue[bytes | None] = queue.Queue()
process = subprocess.Popen(
    ["tail", "-c", f"+{offset + 1}", "-F", path],
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
try:
    matches = 0
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise queue.Empty
        line = lines.get(timeout=remaining)
        if line is None:
            raise SystemExit("log follower ended before the event")
        text = line.decode("utf-8", errors="replace").rstrip()
        if pattern.search(text):
            print(text)
            matches += 1
            if matches >= required_matches:
                break
except queue.Empty:
    raise SystemExit(
        f"timed out after {timeout:g}s waiting for {required_matches} log event(s): "
        f"{pattern.pattern}"
    )
finally:
    process.terminate()
    try:
        process.wait(timeout=2)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()
