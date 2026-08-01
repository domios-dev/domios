#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

python3 - "$ROOT" <<'PY'
from pathlib import Path
import re
import sys

root = Path(sys.argv[1])
routes_rs = root / "src-tauri/src/api/routes.rs"
api_md = root / "docs/API.md"

internal_prefixes = (
    "/api/debug/",
    "/api/events/",
    "/api/mux/",
    "/api/pty/",
    "/api/upload",
    "/api/transcribe-voice",
    "/web",
)
internal_exact = {
    "/api/tabs/{id}/resize-owner",
    "/api/tabs/{id}/chat-view",
    "/api/tabs/{id}/chat-raw/start",
    "/api/tabs/{id}/chat-raw/stop",
    "/api/tabs/{id}/chat-raw/backfill",
    "/api/workspaces/{id}/tabs/{tab_id}/focus",
    "/api/workspaces/{id}/tabs/{tab_id}/close",
    "/api/mesh/peers/{id}/services",
}

def normalize(path: str) -> str:
    path = path.strip()
    path = re.sub(r"\{tid\}", "{tab_id}", path)
    path = re.sub(r"\{[^}/]+\}", "{}", path)
    path = re.sub(r"/:[^/]+", "/{}", path)
    return path

def route_calls(text: str):
    needle = ".route("
    i = 0
    while True:
        start = text.find(needle, i)
        if start == -1:
            return
        pos = start + len(needle)
        depth = 1
        in_str = False
        esc = False
        while pos < len(text) and depth:
            ch = text[pos]
            if in_str:
                if esc:
                    esc = False
                elif ch == "\\":
                    esc = True
                elif ch == '"':
                    in_str = False
            else:
                if ch == '"':
                    in_str = True
                elif ch == "(":
                    depth += 1
                elif ch == ")":
                    depth -= 1
            pos += 1
        yield text[start + len(needle):pos - 1]
        i = pos

registered = set()
routes_text = routes_rs.read_text(encoding="utf-8").split("#[cfg(test)]", 1)[0]
for call in route_calls(routes_text):
    m = re.match(r'\s*"([^"]+)"\s*,\s*(.*)\s*$', call, re.S)
    if not m:
        continue
    path, expr = m.groups()
    methods = set(re.findall(r"(?:^|[.\s])(?:axum::routing::)?(get|post|put|patch|delete)\s*\(", expr))
    for method in methods:
        registered.add((method.upper(), path))

documented = set()
for line in api_md.read_text(encoding="utf-8").splitlines():
    m = re.match(r"\|\s*(GET|POST|PUT|PATCH|DELETE)\s*\|\s*`([^`]+)`", line)
    if m:
        documented.add((m.group(1), m.group(2)))

def is_internal(route):
    _, path = route
    return path in internal_exact or any(path.startswith(prefix) for prefix in internal_prefixes)

reg_norm = {(m, normalize(p)): (m, p) for m, p in registered if not is_internal((m, p))}
doc_norm = {(m, normalize(p)): (m, p) for m, p in documented if not is_internal((m, p))}

missing = [reg_norm[k] for k in sorted(reg_norm.keys() - doc_norm.keys())]
stale = [doc_norm[k] for k in sorted(doc_norm.keys() - reg_norm.keys())]

print(f"registered public routes: {len(reg_norm)}")
print(f"documented routes: {len(doc_norm)}")

if missing:
    print("\nImplemented but undocumented:")
    for method, path in missing:
        print(f"{method} {path}")

if stale:
    print("\nDocumented but unregistered:")
    for method, path in stale:
        print(f"{method} {path}")

if not missing and not stale:
    print("\nAPI docs are in sync with registered public routes.")
PY
