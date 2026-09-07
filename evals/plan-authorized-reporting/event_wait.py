import json
import socket
import sys
import urllib.error
import urllib.request


api_url, token, event_type, marker, output_path, timeout = sys.argv[1:]
request = urllib.request.Request(
    f"{api_url}/api/events/stream",
    headers={"Authorization": f"Bearer {token}"},
)
try:
    with urllib.request.urlopen(request, timeout=float(timeout)) as response:
        print("READY", flush=True)
        for raw in response:
            line = raw.decode("utf-8").strip()
            if not line.startswith("data: "):
                continue
            event = json.loads(line[6:])
            if event.get("event_type") != event_type:
                continue
            if marker not in json.dumps(event.get("data"), sort_keys=True):
                continue
            with open(output_path, "w", encoding="utf-8") as output:
                json.dump(event, output)
            raise SystemExit(0)
except (TimeoutError, socket.timeout, urllib.error.URLError):
    print(f"event deadline expired: {event_type}/{marker}", file=sys.stderr)
    raise SystemExit(1)

print(f"event stream ended: {event_type}/{marker}", file=sys.stderr)
raise SystemExit(1)
