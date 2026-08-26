#!/usr/bin/env python3
"""Resolve the status-bar band a replay's provider actually repaints.

`render_timeline` requires `--status-bar-rows`: replaying with a band the
provider does not have renders a timeline for a program nobody runs. The band
comes from the provider's own plugin manifest, never from a baked number.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def rows_for_provider(provider: str) -> int | None:
    manifest = ROOT / "plugins" / f"provider-{provider}" / "plugin.json"
    if not manifest.exists():
        return None
    spawn = json.loads(manifest.read_text(encoding="utf-8")).get("spawn", {})
    rows = spawn.get("statusBarRows")
    return None if rows is None else int(rows)


def provider_for_replay(replay_dir: Path) -> str | None:
    meta = replay_dir / "meta.json"
    if meta.exists():
        declared = json.loads(meta.read_text(encoding="utf-8")).get("provider")
        if declared:
            return str(declared)
    parts = replay_dir.resolve().parts
    if "replays" in parts:
        idx = len(parts) - 1 - parts[::-1].index("replays")
        if idx + 1 < len(parts):
            return parts[idx + 1]
    return None


def rows_for_replay(replay_dir: Path, fixture_rows: int | None = None) -> int:
    meta = replay_dir / "meta.json"
    if meta.exists():
        declared = json.loads(meta.read_text(encoding="utf-8")).get("status_bar_rows")
        if declared is not None:
            return int(declared)
    provider = provider_for_replay(replay_dir)
    if provider is None:
        raise SystemExit(f"{replay_dir}: no provider declared and none inferable")
    rows = rows_for_provider(provider)
    if rows is not None:
        return rows
    if fixture_rows is None:
        raise SystemExit(
            f"{replay_dir}: provider {provider!r} has no plugin manifest; "
            "declare the band with --fixture-rows"
        )
    return fixture_rows


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("replay_dir", type=Path)
    ap.add_argument(
        "--fixture-rows",
        type=int,
        default=None,
        help="explicit band for a cast whose provider has no plugin manifest (synthetic fixtures)",
    )
    args = ap.parse_args()
    print(rows_for_replay(args.replay_dir, args.fixture_rows))


if __name__ == "__main__":
    main()
