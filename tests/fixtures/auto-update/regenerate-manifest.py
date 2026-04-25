#!/usr/bin/env python3
"""Rebuild manifest.json from real signed assets in dist/.

Reads each `*.sig` file produced by `cargo tauri build` and inlines the
base64 signature into the manifest. Asset URLs point at the local
http.server (`http://127.0.0.1:8080/dist/<filename>`) by default; pass
`--base-url` to override.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


PLATFORM_BY_SUFFIX = {
    "x64.msi.zip": "windows-x86_64",
    "aarch64.app.tar.gz": "darwin-aarch64",
    "x64.app.tar.gz": "darwin-x86_64",
    "x86_64.app.tar.gz": "darwin-x86_64",
}


def detect_platform(name: str) -> str | None:
    for suffix, key in PLATFORM_BY_SUFFIX.items():
        if name.endswith(suffix):
            return key
    return None


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--dist", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--base-url", default="http://127.0.0.1:8080/dist")
    parser.add_argument("--notes", default="CodeTerm test fixture build")
    args = parser.parse_args()

    if not args.dist.is_dir():
        print(f"dist directory not found: {args.dist}", file=sys.stderr)
        return 1

    platforms: dict[str, dict[str, str]] = {}
    for sig_file in sorted(args.dist.glob("*.sig")):
        asset_name = sig_file.name[:-4]
        asset_path = args.dist / asset_name
        if not asset_path.exists():
            continue
        platform = detect_platform(asset_name)
        if platform is None:
            print(f"skip (unknown platform suffix): {asset_name}", file=sys.stderr)
            continue
        signature = sig_file.read_text().strip()
        platforms[platform] = {
            "url": f"{args.base_url}/{asset_name}",
            "signature": signature,
        }

    if not platforms:
        print("no signed assets found in dist/", file=sys.stderr)
        return 1

    manifest = {
        "version": args.version,
        "notes": args.notes,
        "pub_date": "1970-01-01T00:00:00Z",
        "platforms": platforms,
    }
    args.output.write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"wrote {args.output} with {len(platforms)} platform(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
