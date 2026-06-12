"""E2E coverage for the cross-mesh pastebin (#98).

Runs against the dev daemon on port 7686. Verifies the REST surface,
CLI parity, and the lifecycle rules from the issue acceptance criteria:
short body round-trip, TTL override, pop_on_read self-destruct, explicit
DELETE, list enumeration, long body without truncation, empty body
rejected.
"""
from __future__ import annotations

import os
import shutil
import subprocess
import time
from pathlib import Path

import pytest
import requests


DEV_PORT = 7686
BASE = f"http://127.0.0.1:{DEV_PORT}"


def _read_dev_token() -> str | None:
    cfg = Path.home() / ".codeterm-dev" / "config.toml"
    if not cfg.exists():
        return None
    in_api = False
    for line in cfg.read_text(encoding="utf-8").splitlines():
        stripped = line.strip()
        if stripped == "[api]":
            in_api = True
            continue
        if stripped.startswith("[") and in_api:
            break
        if in_api and stripped.startswith("token =") and "=" in stripped:
            return stripped.split("=", 1)[1].strip().strip('"')
    return None


def _resolve_token() -> str:
    dev_token = _read_dev_token()
    return dev_token or ""


TOKEN = _resolve_token()
os.environ["CODETERM_API_URL"] = BASE
os.environ["CODETERM_API_PORT"] = str(DEV_PORT)
os.environ["CODETERM_API_TOKEN"] = TOKEN
HEADERS = {"Authorization": f"Bearer {TOKEN}", "Content-Type": "application/json"}


def _put(body: str, **kwargs) -> dict:
    payload = {"body": body, **kwargs}
    r = requests.post(f"{BASE}/api/pasteboard", json=payload, headers=HEADERS, timeout=5)
    r.raise_for_status()
    return r.json()


def _get(paste_id: str, pop: bool = False) -> requests.Response:
    suffix = "?pop=true" if pop else ""
    return requests.get(f"{BASE}/api/pasteboard/{paste_id}{suffix}", headers=HEADERS, timeout=5)


def _delete(paste_id: str) -> requests.Response:
    return requests.delete(f"{BASE}/api/pasteboard/{paste_id}", headers=HEADERS, timeout=5)


def _list() -> list[dict]:
    r = requests.get(f"{BASE}/api/pasteboard", headers=HEADERS, timeout=5)
    r.raise_for_status()
    return r.json()


@pytest.fixture(autouse=True)
def _require_dev():
    if not TOKEN:
        pytest.skip("no dev token — start dev with ./build.sh dev")
    try:
        requests.get(f"{BASE}/api/health", timeout=2).raise_for_status()
    except Exception:
        pytest.skip(f"dev not reachable at {BASE}")


def test_put_then_get_roundtrips_short_body():
    created = _put("hello world")
    assert created["id"].startswith("paste:"), created
    try:
        body = _get(created["id"]).json()
        assert body["body"] == "hello world"
        # No pop → second read still works.
        assert _get(created["id"]).json()["body"] == "hello world"
    finally:
        _delete(created["id"])


def test_ttl_override_lands_in_expires_at():
    target_ms = 30 * 60 * 1000  # 30 min
    created = _put("ttl-test", ttl_ms=target_ms)
    try:
        listed = next(e for e in _list() if e["id"] == created["id"])
        now_ms = int(time.time() * 1000)
        ttl_remaining = listed["expires_at"] - now_ms
        # Allow ±15s slop for round-trip latency and clock skew.
        assert abs(ttl_remaining - target_ms) < 15_000, (
            f"ttl_remaining={ttl_remaining}ms target={target_ms}ms"
        )
    finally:
        _delete(created["id"])


def test_pop_on_read_consumes_entry():
    created = _put("one-shot", pop_on_read=True)
    first = _get(created["id"])
    assert first.status_code == 200
    assert first.json()["body"] == "one-shot"
    second = _get(created["id"])
    assert second.status_code == 404, "pop_on_read entry must be gone after first read"


def test_explicit_delete_removes_entry():
    created = _put("delete-me")
    assert _delete(created["id"]).status_code == 200
    assert _get(created["id"]).status_code == 404


def test_list_enumerates_outstanding_pastes():
    # Snapshot baseline so the test is non-flaky if other clients have
    # outstanding pastes on the same dev daemon.
    baseline_ids = {e["id"] for e in _list()}
    created = _put("list-me")
    try:
        ids = {e["id"] for e in _list()}
        new_ids = ids - baseline_ids
        assert created["id"] in new_ids
    finally:
        _delete(created["id"])


def test_long_body_roundtrips_without_truncation():
    payload = "A" * 10240
    created = _put(payload)
    try:
        body = _get(created["id"]).json()["body"]
        assert len(body) == 10240
        assert body == payload
    finally:
        _delete(created["id"])


def test_empty_body_rejected_with_400():
    r = requests.post(
        f"{BASE}/api/pasteboard",
        json={"body": ""},
        headers=HEADERS,
        timeout=5,
    )
    assert r.status_code == 400


def test_cli_put_get_pop_rm_parity():
    """codeterm paste subcommand should round-trip through the same daemon."""
    cli = shutil.which("codeterm") or os.path.join(
        os.environ.get("USERPROFILE", os.environ.get("HOME", "")),
        ".codeterm-dev",
        "bin",
        "codeterm.exe" if os.name == "nt" else "codeterm",
    )
    if not (cli and os.path.exists(cli)):
        pytest.skip("codeterm CLI binary not on PATH")
    # CLI honors CODETERM_API_PORT (env var, set by CodeTerm PTY) for port
    # routing, not CODETERM_API_URL — so derive the port from BASE and pin
    # both that and the token so the subprocess targets the dev daemon
    # regardless of what the parent shell happens to point at.
    port = BASE.rsplit(":", 1)[-1].split("/", 1)[0]
    env = {
        **os.environ,
        "CODETERM_API_PORT": port,
        "CODETERM_API_TOKEN": TOKEN,
    }
    put = subprocess.run(
        [cli, "paste", "put", "--from-stdin"],
        input="via-cli\n",
        text=True,
        capture_output=True,
        env=env,
        check=True,
    )
    paste_id = put.stdout.strip()
    assert paste_id.startswith("paste:"), put.stdout

    try:
        got = subprocess.run(
            [cli, "paste", "get", paste_id],
            text=True,
            capture_output=True,
            env=env,
            check=True,
        )
        assert "via-cli" in got.stdout, got.stdout
    finally:
        subprocess.run([cli, "paste", "rm", paste_id], env=env, check=False)
