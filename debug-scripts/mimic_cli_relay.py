"""Replicate exactly what `codeterm pane list --mesh PEER` does at HTTP level.

The CLI in mesh_relay_generic does:
  1. GET {local}/api/mesh/peers with Authorization: Bearer {signed}
     (Authorization is on default_headers via build_http())
  2. POST {peer}/api/mesh/relay with Authorization: Bearer {signed}
     PLUS the same default_headers Authorization (reqwest behavior:
     default_headers + .header() may stack)
     Body: {"method":"GET","path":"/api/panes","body":null}
  3. Parse response as serde_json::Value, unwrap_or_default(); if json mode
     print pretty.

We test:
  - Plain Authorization (works in PyNaCl probe).
  - Stacked Authorization (two identical Bearer headers).
  - Verify what response we get back.
"""
from __future__ import annotations

import base64
import json
import sys
import time
import uuid
from pathlib import Path

from nacl.signing import SigningKey  # type: ignore[import-not-found]


def b64encode(data: bytes) -> str:
    return base64.b64encode(data).decode("ascii")


def sign_token(private_key_b64: str, instance_id: str) -> str:
    sk = SigningKey(base64.b64decode(private_key_b64))
    payload = {
        "from_instance_id": instance_id,
        "timestamp": int(time.time()),
        "nonce": str(uuid.uuid4()),
    }
    payload_b64 = b64encode(json.dumps(payload).encode())
    signed = sk.sign(payload_b64.encode())
    return f"{payload_b64}.{b64encode(signed.signature)}"


def main() -> int:
    ident = json.loads((Path.home() / ".codeterm" / "mesh" / "identity.json").read_text())
    token = sign_token(ident["private_key"], ident["instance_id"])

    # 1) GET local /api/mesh/peers (CLI does this to resolve peer addr)
    import urllib.request
    print("== Test A: local /api/mesh/peers ==")
    req = urllib.request.Request(
        "http://127.0.0.1:7685/api/mesh/peers",
        headers={"Authorization": f"Bearer {token}"},
    )
    with urllib.request.urlopen(req, timeout=5) as r:
        data = json.loads(r.read())
    paired = data.get("paired", [])
    mac = next((p for p in paired if p["instance_name"] == "Andreys-MacBook-Air-3"), None)
    assert mac, "Mac not paired locally"
    peer_url = f"http://{mac['addresses'][0]['ip']}:{mac['api_port']}"
    print(f"   peer_url = {peer_url}")

    # 2) POST peer /api/mesh/relay with single Authorization
    print("\n== Test B: POST peer /api/mesh/relay  (single Authorization) ==")
    relay_body = json.dumps({"method": "GET", "path": "/api/panes", "body": None}).encode()
    req = urllib.request.Request(
        f"{peer_url}/api/mesh/relay",
        data=relay_body,
        headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=15) as r:
            print(f"   HTTP {r.status}, content-type={r.headers.get('content-type')}")
            body = r.read()
            print(f"   body[:200]: {body[:200]!r}")
            try:
                parsed = json.loads(body)
                print(f"   parsed keys: {list(parsed.keys())}")
                print(f"   ok={parsed.get('ok')}, status={parsed.get('status')}, error={parsed.get('error')}")
                if isinstance(parsed.get("data"), list):
                    print(f"   data is list of {len(parsed['data'])} items")
                elif isinstance(parsed.get("data"), dict):
                    print(f"   data dict keys: {list(parsed['data'].keys())[:6]}")
            except Exception as e:
                print(f"   json parse failed: {e}")
    except Exception as e:
        print(f"   request failed: {e}")

    # 3) POST peer /api/mesh/relay WITH duplicate Authorization (stacked headers)
    print("\n== Test C: POST peer /api/mesh/relay  (duplicate Authorization) ==")
    import http.client
    conn = http.client.HTTPConnection(mac["addresses"][0]["ip"], mac["api_port"], timeout=15)
    conn.putrequest("POST", "/api/mesh/relay")
    conn.putheader("Authorization", f"Bearer {token}")
    conn.putheader("Authorization", f"Bearer {token}")  # second header
    conn.putheader("Content-Type", "application/json")
    conn.putheader("Content-Length", str(len(relay_body)))
    conn.endheaders()
    conn.send(relay_body)
    resp = conn.getresponse()
    print(f"   HTTP {resp.status}, content-type={resp.getheader('content-type')}")
    body = resp.read()
    print(f"   body[:200]: {body[:200]!r}")
    conn.close()

    # 4) Same as B but with longer timeout to rule out timing
    print("\n== Test D: same as B with /api/mesh/peers (smaller payload) ==")
    relay_body_sm = json.dumps({"method": "GET", "path": "/api/mesh/peers", "body": None}).encode()
    req = urllib.request.Request(
        f"{peer_url}/api/mesh/relay",
        data=relay_body_sm,
        headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=10) as r:
        print(f"   HTTP {r.status}, len={len(r.read())}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
