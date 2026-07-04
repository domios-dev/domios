#!/usr/bin/env python3
"""Drop dangling // lines immediately before code (truncated comment tails)."""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SKIP = {"target", "node_modules", "dist", "build", "gen", "references", "vendor", "pty-fork", ".git"}
EXTS = {".rs", ".ts", ".tsx"}
COMPLETE = re.compile(r"(?:[.!?;:»\"'\)]|\*/)\s*$|// ──|// =+$|// -+$")
EXEMPT = re.compile(r"^//(!|/)|eslint-disable|@ts-|NOSONAR|prettier-ignore")


def is_comment_line(s: str) -> bool:
    t = s.lstrip()
    return t.startswith("//") and not EXEMPT.search(t)


def is_complete_comment(s: str) -> bool:
    t = s.lstrip()[2:].strip()
    if not t:
        return False
    return bool(COMPLETE.search(t))


def next_meaningful(lines: list[str], i: int) -> str | None:
    for j in range(i + 1, len(lines)):
        if lines[j].strip():
            return lines[j]
    return None


def clean_file(text: str) -> tuple[str, int]:
    lines = text.splitlines(keepends=True)
    drop: set[int] = set()
    i = 0
    while i < len(lines):
        if not is_comment_line(lines[i]):
            i += 1
            continue
        j = i
        while j < len(lines) and is_comment_line(lines[j]):
            j += 1
        nxt = lines[j] if j < len(lines) else ""
        nxt_stripped = nxt.lstrip()
        is_code = bool(nxt_stripped) and not nxt_stripped.startswith("//")
        if is_code:
            for k in range(i, j):
                if not is_complete_comment(lines[k]):
                    drop.add(k)
        i = j if j > i else i + 1
    if not drop:
        return text, 0
    out = [ln for idx, ln in enumerate(lines) if idx not in drop]
    result = "".join(out)
    if text.endswith("\n") and not result.endswith("\n"):
        result += "\n"
    return result, len(drop)


def main() -> int:
    dry = "--dry-run" in sys.argv
    total = 0
    changed = 0
    for ext in EXTS:
        for p in ROOT.rglob(f"*{ext}"):
            if any(s in SKIP for s in p.parts):
                continue
            orig = p.read_text(encoding="utf-8")
            updated, n = clean_file(orig)
            if n:
                total += n
                changed += 1
                print(f"{p.relative_to(ROOT)}\t{n}")
                if not dry:
                    p.write_text(updated, encoding="utf-8")
    print(f"{'would drop' if dry else 'dropped'} {total} dangling lines in {changed} files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
