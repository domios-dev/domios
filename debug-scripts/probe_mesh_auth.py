"""Probe mesh peer auth with a properly signed Ed25519 token.

Reads ~/.codeterm/mesh/identity.json (BLACKSTAR's identity), constructs a signed
mesh request token in the same format as `cli/src/mesh_cli.rs::sign_request` and
`src-tauri/src/mesh/crown.rs::sign_request`, then POSTs to the peer's mesh_relay
endpoint to capture the raw HTTP status + body. The CLI swallows 401s when the
body is plain text, so we go around it.
"""
from __future__ import annotations

import argparse
import base64
import json
import sys
import time
import urllib.error
import urllib.request
import uuid
from pathlib import Path


def b64encode(data: bytes) -> str:
    return base64.b64encode(data).decode("ascii")


def sign_token(private_key_b64: str, instance_id: str) -> str:
    try:
        from nacl.signing import SigningKey  # type: ignore[import-not-found]
    except ImportError:
        print("PyNaCl missing — installing...", file=sys.stderr)
        import subprocess
        subprocess.check_call([sys.executable, "-m", "pip", "install", "--quiet", "pynacl"])
        from nacl.signing import SigningKey  # type: ignore[import-not-found]

    priv_bytes = base64.b64decode(private_key_b64)
    if len(priv_bytes) != 32:
        raise SystemExit(f"private key length {len(priv_bytes)} != 32")
    sk = SigningKey(priv_bytes)

    payload = {
        "from_instance_id": instance_id,
        "timestamp": int(time.time()),
        "nonce": str(uuid.uuid4()),
    }
    payload_bytes = json.dumps(payload).encode("utf-8")
    payload_b64 = b64encode(payload_bytes)
    signed = sk.sign(payload_b64.encode("ascii"))
    sig_b64 = b64encode(signed.signature)
    return f"{payload_b64}.{sig_b64}"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("peer_url", help="e.g. http://100.85.116.96:7685")
    ap.add_argument("--identity", default=str(Path.home() / ".codeterm" / "mesh" / "identity.json"))
    ap.add_argument("--path", default="/api/health")
    ap.add_argument("--method", default="GET")
    args = ap.parse_args()

    ident = json.loads(Path(args.identity).read_text(encoding="utf-8"))
    print(f"[probe] our instance_id: {ident['instance_id']}")
    print(f"[probe] our public_key:  {ident['public_key']}")

    token = sign_token(ident["private_key"], ident["instance_id"])
    print(f"[probe] signed token (len={len(token)}, first 40): {token[:40]}...")

    relay_body = json.dumps({"method": args.method, "path": args.path, "body": None}).encode("utf-8")
    relay_url = f"{args.peer_url.rstrip('/')}/api/mesh/relay"
    print(f"[probe] POST {relay_url} body={relay_body.decode()}")
    req = urllib.request.Request(
        relay_url,
        data=relay_body,
        headers={
            "Content-Type": "application/json",
            "Authorization": f"Bearer {token}",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            print(f"[probe] HTTP {resp.status}")
            body = resp.read().decode("utf-8", errors="replace")
            print(f"[probe] body:\n{body}")
    except urllib.error.HTTPError as e:
        print(f"[probe] HTTPError {e.code} {e.reason}")
        body = e.read().decode("utf-8", errors="replace")
        print(f"[probe] body:\n{body}")
        return e.code
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
