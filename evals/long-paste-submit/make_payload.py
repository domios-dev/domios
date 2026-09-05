import pathlib
import sys


path = pathlib.Path(sys.argv[1])
length = int(sys.argv[2])
token = sys.argv[3]
line = f"{token} long-paste line with deterministic ASCII content\n"
payload = (token + "\n" + line * (length // len(line) + 2))[:length]
if len(payload) != length:
    raise SystemExit(f"wanted {length} characters, made {len(payload)}")
path.write_text(payload, encoding="utf-8", newline="")
print(f"payload={path} chars={len(payload)} bytes={len(payload.encode('utf-8'))}")
