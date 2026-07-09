#!/usr/bin/env python3
"""Remove inline // comment runs of 3+ consecutive lines (AGENTS.md hygiene).

Exempt prefixes match check-comment-length.sh. Only touches .rs/.ts/.tsx outside
target/, node_modules/, dist/, gen/, references/, vendor/, pty-fork/.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SKIP_PARTS = {
    "target",
    "node_modules",
    "dist",
    "build",
    "gen",
    "references",
    "vendor",
    "pty-fork",
    ".git",
}
EXTS = {".rs", ".ts", ".tsx"}

EXEMPT = re.compile(
    r"^//(!|/)|"
    r"^// eslint-disable|"
    r"^// @ts-|"
    r"^// NOSONAR|"
    r"^// prettier-ignore|"
    r"^#\["
)


def should_skip(path: Path) -> bool:
    return any(part in SKIP_PARTS for part in path.parts)


def trim_text(text: str) -> tuple[str, int]:
    lines = text.splitlines(keepends=True)
    out: list[str] = []
    run: list[tuple[int, str]] = []
    removed = 0

    def flush_run() -> None:
        nonlocal removed
        if len(run) >= 3:
            removed += len(run)
            run.clear()
            return
        for _, ln in run:
            out.append(ln)
        run.clear()

    for ln in lines:
        stripped = ln.lstrip()
        if EXEMPT.match(stripped):
            flush_run()
            out.append(ln)
            continue
        if stripped.startswith("//"):
            run.append((len(out), ln))
            continue
        flush_run()
        out.append(ln)
    flush_run()
    result = "".join(out)
    if text.endswith("\n") and not result.endswith("\n"):
        result += "\n"
    return result, removed


def iter_sources() -> list[Path]:
    files: list[Path] = []
    for ext in EXTS:
        for p in ROOT.rglob(f"*{ext}"):
            if should_skip(p):
                continue
            files.append(p)
    return sorted(files)


def expand_targets(targets: list[Path]) -> list[Path]:
    out: list[Path] = []
    for raw in targets:
        p = raw if raw.is_absolute() else ROOT / raw
        if p.is_file() and p.suffix in EXTS and not should_skip(p):
            out.append(p)
            continue
        if p.is_dir():
            for ext in EXTS:
                for f in sorted(p.rglob(f"*{ext}")):
                    if not should_skip(f):
                        out.append(f)
    return out


def main() -> int:
    dry = "--dry-run" in sys.argv
    targets = [Path(a) for a in sys.argv[1:] if not a.startswith("-")]
    paths = expand_targets(targets) if targets else iter_sources()
    total_removed = 0
    changed: list[str] = []
    for p in paths:
        if not p.is_file():
            continue
        rel = p.relative_to(ROOT)
        if should_skip(p):
            continue
        original = p.read_text(encoding="utf-8")
        updated, removed = trim_text(original)
        if removed and updated != original:
            total_removed += removed
            changed.append(f"{rel}\t{removed}")
            if not dry:
                p.write_text(updated, encoding="utf-8")
    for line in changed:
        print(line)
    print(f"{'would remove' if dry else 'removed'} {total_removed} comment lines in {len(changed)} files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
