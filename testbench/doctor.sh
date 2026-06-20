#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SNAP_ROOT="${1:-$ROOT/testbench/snapshots}"

python3 - "$SNAP_ROOT" <<'PY'
import base64
import json
import os
import re
import sys

root = sys.argv[1]
bad = []

FOOTER_RE = re.compile(
    r"(enter\s+(to\s+)?(select|submit)|type to filter|esc\s+dismiss|"
    r"space\s+(to\s+)?select|tab\s+(to\s+)?(select|edit|toggle)|"
    r"↑[/↓]*\s*option|↑↓\s*(to\s+)?(navigate|select))",
    re.I,
)
SPINNER_RE = re.compile(r"[⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏]|thinking…|thinking\.\.\.|esc to interrupt", re.I)
VISIBLE_SELECTION_RE = re.compile(
    r"(?m)^(?:\s*(?:[→●◉◆✓✔☑☒☐]|\[[ xX]\]|\([ xX]\))\s+\S|"
    r"\s*[❯›]\s+(?:\d+[.)]|\[[ xX]\]|\([ xX]\)|[A-Z][.)]))"
)
BG_OR_INVERSE_RE = re.compile(rb"\x1b\[[0-9;:]*7m|\x1b\[[0-9;:]*48[;:]")

def norm(s):
    return re.sub(r"\s+", " ", s).strip()

def read(path):
    with open(path, "r", encoding="utf-8", errors="replace") as f:
        return f.read()

def cast_bytes(path):
    out = bytearray()
    try:
        with open(path, "r", encoding="utf-8") as f:
            next(f, None)
            for line in f:
                line = line.strip()
                if not line:
                    continue
                row = json.loads(line)
                if len(row) < 3:
                    continue
                if row[1] == "o":
                    out.extend(row[2].encode("utf-8"))
                elif row[1] == "o+b64":
                    out.extend(base64.b64decode(row[2]))
    except Exception:
        return b""
    return bytes(out)

def strings_in_expected(value):
    found = []
    def walk(v):
        if isinstance(v, str):
            s = norm(v)
            if len(s) > 1 and s not in {"provider_screen", "agent_state_replay", "PermissionRequest"}:
                found.append(s)
        elif isinstance(v, list):
            for item in v:
                walk(item)
        elif isinstance(v, dict):
            for key, item in v.items():
                if key in {"provider", "unit", "source", "final_state", "negatives", "value"}:
                    continue
                walk(item)
    if isinstance(value, dict) and "expected" in value:
        walk(value["expected"])
    return found

def is_pickerish(name):
    return any(part in name.lower() for part in ("picker", "wizard", "options", "question"))

for provider in sorted(os.listdir(root)) if os.path.isdir(root) else []:
    pdir = os.path.join(root, provider)
    if not os.path.isdir(pdir):
        continue
    for situation in sorted(os.listdir(pdir)):
        sdir = os.path.join(pdir, situation)
        if not os.path.isdir(sdir):
            continue
        rel = f"{provider}/{situation}"
        reasons = []
        screen_path = os.path.join(sdir, "screen.txt")
        cast_path = os.path.join(sdir, "raw.cast")
        expected_path = os.path.join(sdir, "expected.json")
        screen = read(screen_path) if os.path.exists(screen_path) else ""
        flat = norm(screen)
        expected_grounded = False

        if not os.path.exists(cast_path):
            reasons.append("raw.cast missing")

        if os.path.exists(expected_path) and screen:
            expected = json.load(open(expected_path, encoding="utf-8"))
            missing = [s for s in strings_in_expected(expected) if s not in flat]
            if missing:
                reasons.append("expected literal absent: " + "; ".join(missing[:5]))
            else:
                expected_grounded = True

        if screen and SPINNER_RE.search(screen) and not FOOTER_RE.search(screen):
            reasons.append("mid-animation/spinner-looking frame")

        if is_pickerish(situation) and screen:
            raw = cast_bytes(cast_path) if os.path.exists(cast_path) else b""
            has_selection = bool(VISIBLE_SELECTION_RE.search(screen)) or (
                provider in {"cursor", "opencode"}
                and expected_grounded
                and bool(BG_OR_INVERSE_RE.search(raw))
            )
            has_footer = bool(FOOTER_RE.search(screen))
            if not has_selection and not has_footer:
                reasons.append("picker-like situation has no selection signal and no footer hint")

        if reasons:
            bad.append((rel, reasons))

if bad:
    print("BAD snapshots:")
    for rel, reasons in bad:
        print(f"- {rel}: " + "; ".join(reasons))
    sys.exit(1)

print("doctor: ok")
PY
