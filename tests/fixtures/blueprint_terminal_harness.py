import hashlib
import sys

print("harness ready", flush=True)
for line in sys.stdin.buffer:
    body = line.rstrip(b"\r\n")
    digest = hashlib.sha256(body).hexdigest()
    print(f"reply bytes={len(body)} sha256={digest}", flush=True)
