#!/usr/bin/env python3
"""Build a single-platform updater fragment for the Tauri channel manifest.

Inputs come from environment variables so the workflow doesn't need to do
any shell-quoting gymnastics around the binary signature blob:

  TAG             — git tag, e.g. "v1.1.1-rc3" (leading 'v' is stripped for the
                    fragment's `version` field).
  REPO            — "owner/repo".
  PLATFORM_KEY    — Tauri updater platform key, e.g. "windows-x86_64",
                    "linux-x86_64", "darwin-x86_64", "darwin-aarch64".
  UPDATER_BUNDLE  — path to the file users will download (e.g. *.nsis.zip,
                    *.AppImage.tar.gz, *.app.tar.gz). Only the basename ends up
                    in the URL.
  UPDATER_SIG     — path to the signature file (.sig) produced by Tauri's
                    bundler next to UPDATER_BUNDLE. Its full text content goes
                    verbatim into the fragment's `signature` field.
  OUT             — output path for the fragment JSON.
"""

from __future__ import annotations

import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path


REQUIRED = ("TAG", "REPO", "PLATFORM_KEY", "UPDATER_BUNDLE", "UPDATER_SIG", "OUT")


def main() -> int:
    missing = [k for k in REQUIRED if not os.environ.get(k)]
    if missing:
        print(f"::error::missing env vars: {', '.join(missing)}", file=sys.stderr)
        return 1

    tag = os.environ["TAG"]
    repo = os.environ["REPO"]
    platform_key = os.environ["PLATFORM_KEY"]
    bundle_path = Path(os.environ["UPDATER_BUNDLE"])
    sig_path = Path(os.environ["UPDATER_SIG"])
    out_path = Path(os.environ["OUT"])

    signature = sig_path.read_text(encoding="utf-8").strip()
    if not signature:
        print(f"::error::signature file is empty: {sig_path}", file=sys.stderr)
        return 1

    version = tag[1:] if tag.startswith("v") else tag
    pub_date = datetime.now(tz=timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    asset_url = (
        f"https://github.com/{repo}/releases/download/{tag}/{bundle_path.name}"
    )

    fragment = {
        "version": version,
        "notes": "See the assets to download this version and install.",
        "pub_date": pub_date,
        "platforms": {
            platform_key: {
                "url": asset_url,
                "signature": signature,
            },
        },
    }

    out_path.write_text(json.dumps(fragment, indent=2) + "\n", encoding="utf-8")
    print(f"Wrote {out_path} for {platform_key} -> {bundle_path.name}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
