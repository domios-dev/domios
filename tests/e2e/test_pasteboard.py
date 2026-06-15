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

import pytest
import requests


def _token(api_only) -> str:
    return api_only.s.headers["Authorization"].removeprefix("Bearer ")


def _headers(api_only) -> dict[str, str]:
    return {
        "Authorization": f"Bearer {_token(api_only)}",
        "Content-Type": "application/json",
    }


def _put(api_only, body: str, **kwargs) -> dict:
    payload = {"body": body, **kwargs}
    r = requests.post(
        f"{api_only.base}/api/pasteboard",
        json=payload,
        headers=_headers(api_only),
        timeout=5,
    )
    r.raise_for_status()
    return r.json()


def _get(api_only, paste_id: str, pop: bool = False) -> requests.Response:
    suffix = "?pop=true" if pop else ""
    return requests.get(
        f"{api_only.base}/api/pasteboard/{paste_id}{suffix}",
        headers=_headers(api_only),
        timeout=5,
    )


def _delete(api_only, paste_id: str) -> requests.Response:
    return requests.delete(
        f"{api_only.base}/api/pasteboard/{paste_id}",
        headers=_headers(api_only),
        timeout=5,
    )


def _list(api_only) -> list[dict]:
    r = requests.get(
        f"{api_only.base}/api/pasteboard",
        headers=_headers(api_only),
        timeout=5,
    )
    r.raise_for_status()
    return r.json()


def test_put_then_get_roundtrips_short_body(api_only):
    created = _put(api_only, "hello world")
    assert created["id"].startswith("paste:"), created
    try:
        body = _get(api_only, created["id"]).json()
        assert body["body"] == "hello world"
        # No pop → second read still works.
        assert _get(api_only, created["id"]).json()["body"] == "hello world"
    finally:
        _delete(api_only, created["id"])


def test_ttl_override_lands_in_expires_at(api_only):
    target_ms = 30 * 60 * 1000  # 30 min
    created = _put(api_only, "ttl-test", ttl_ms=target_ms)
    try:
        listed = next(e for e in _list(api_only) if e["id"] == created["id"])
        now_ms = int(time.time() * 1000)
        ttl_remaining = listed["expires_at"] - now_ms
        # Allow ±15s slop for round-trip latency and clock skew.
        assert abs(ttl_remaining - target_ms) < 15_000, (
            f"ttl_remaining={ttl_remaining}ms target={target_ms}ms"
        )
    finally:
        _delete(api_only, created["id"])


def test_pop_on_read_consumes_entry(api_only):
    created = _put(api_only, "one-shot", pop_on_read=True)
    first = _get(api_only, created["id"])
    assert first.status_code == 200
    assert first.json()["body"] == "one-shot"
    second = _get(api_only, created["id"])
    assert second.status_code == 404, "pop_on_read entry must be gone after first read"


def test_explicit_delete_removes_entry(api_only):
    created = _put(api_only, "delete-me")
    assert _delete(api_only, created["id"]).status_code == 200
    assert _get(api_only, created["id"]).status_code == 404


def test_list_enumerates_outstanding_pastes(api_only):
    # Snapshot baseline so the test is non-flaky if other clients have
    # outstanding pastes on the same dev daemon.
    baseline_ids = {e["id"] for e in _list(api_only)}
    created = _put(api_only, "list-me")
    try:
        ids = {e["id"] for e in _list(api_only)}
        new_ids = ids - baseline_ids
        assert created["id"] in new_ids
    finally:
        _delete(api_only, created["id"])


def test_long_body_roundtrips_without_truncation(api_only):
    payload = "A" * 10240
    created = _put(api_only, payload)
    try:
        body = _get(api_only, created["id"]).json()["body"]
        assert len(body) == 10240
        assert body == payload
    finally:
        _delete(api_only, created["id"])


def test_empty_body_rejected_with_400(api_only):
    r = requests.post(
        f"{api_only.base}/api/pasteboard",
        json={"body": ""},
        headers=_headers(api_only),
        timeout=5,
    )
    assert r.status_code == 400


def test_cli_put_get_pop_rm_parity(api_only):
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
    port = api_only.base.rsplit(":", 1)[-1].split("/", 1)[0]
    env = {
        **os.environ,
        "CODETERM_API_PORT": port,
        "CODETERM_API_TOKEN": _token(api_only),
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
