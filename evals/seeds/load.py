#!/usr/bin/env python3
"""
Load loaded.py items into the running dev instance.

Usage:
  python3 load.py                # uses http://127.0.0.1:7686
  python3 load.py --api URL --token TOK
"""

import argparse
import json
import sys
import time
import urllib.request
import urllib.error
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from loaded import ITEMS  # noqa: E402


def read_token() -> str:
    cfg = Path.home() / ".codeterm-dev" / "config.toml"
    if not cfg.exists():
        sys.exit(f"no dev config at {cfg}")
    for line in cfg.read_text().splitlines():
        line = line.strip()
        if line.startswith("token"):
            return line.split("=", 1)[1].strip().strip('"').strip("'")
    sys.exit("no token in config.toml")


def post(api: str, token: str, path: str, body: dict, timeout: float = 30.0) -> dict:
    req = urllib.request.Request(
        f"{api}{path}",
        data=json.dumps(body).encode("utf-8"),
        headers={
            "Authorization": f"Bearer {token}",
            "Content-Type": "application/json",
        },
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.loads(resp.read().decode("utf-8"))


def get(api: str, token: str, path: str, timeout: float = 10.0) -> dict:
    req = urllib.request.Request(
        f"{api}{path}",
        headers={"Authorization": f"Bearer {token}"},
    )
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.loads(resp.read().decode("utf-8"))


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--api", default="http://127.0.0.1:7686")
    ap.add_argument("--token", default=None)
    ap.add_argument("--wait-workers", type=int, default=120,
                    help="seconds to wait for extraction workers to drain (0 to skip)")
    ap.add_argument("--sleep", type=float, default=0.02,
                    help="sleep between saves (seconds)")
    ns = ap.parse_args()

    token = ns.token or read_token()

    # Health check
    try:
        health = get(ns.api, token, "/api/health")
        print(f"[load] dev ready: build={health.get('build_number')} commit={health.get('build')}")
    except Exception as e:
        sys.exit(f"[load] dev not reachable at {ns.api}: {e}")

    total = len(ITEMS)
    print(f"[load] posting {total} items...")
    ok = 0
    errors = 0
    t0 = time.time()
    for i, item in enumerate(ITEMS, 1):
        try:
            resp = post(ns.api, token, "/api/mem/items", item)
            if "id" in resp:
                ok += 1
                if i % 20 == 0 or i == total:
                    elapsed = time.time() - t0
                    print(f"[load] {i}/{total} ok={ok} err={errors} ({elapsed:.1f}s)")
            else:
                errors += 1
                print(f"[load]  ! no id in response for item {i}: {resp}")
        except urllib.error.HTTPError as e:
            errors += 1
            print(f"[load]  ! {e.code} for item {i}: {e.read().decode('utf-8', errors='replace')}")
        except Exception as e:
            errors += 1
            print(f"[load]  ! {type(e).__name__} for item {i}: {e}")
        time.sleep(ns.sleep)

    elapsed = time.time() - t0
    print(f"[load] done: {ok}/{total} ok, {errors} errors, {elapsed:.1f}s")

    # Wait for extraction workers AND bg embedding threads to drain.
    # Condition: item count stable for 2 consecutive polls AND vectors >= items - 2
    # (the -2 slack covers bundled-identity which may lack vectors).
    if ns.wait_workers > 0:
        print(f"[load] waiting for indexing to settle (max {ns.wait_workers}s)...")
        deadline = time.time() + ns.wait_workers
        prev_items = -1
        stable = 0
        while time.time() < deadline:
            try:
                stats = get(ns.api, token, "/api/mem/stats")
                items = stats.get("total_items", 0)
                vectors = stats.get("total_vectors", 0)
                if items == prev_items:
                    stable += 1
                else:
                    stable = 0
                prev_items = items
                gap = items - vectors
                print(f"[load]   items={items} vectors={vectors} gap={gap} stable={stable}")
                if stable >= 2 and gap <= 3:
                    print("[load] indexing settled")
                    break
                time.sleep(3)
            except Exception as e:
                print(f"[load]  ! stats poll failed: {e}")
                time.sleep(3)
        else:
            print("[load]  ! settle wait timed out, continuing anyway")

    # Final stats
    try:
        stats = get(ns.api, token, "/api/mem/stats")
        print("\n[load] final stats:")
        print(json.dumps(stats, indent=2))
    except Exception as e:
        print(f"[load]  ! stats fetch failed: {e}")

    return 0 if errors == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
