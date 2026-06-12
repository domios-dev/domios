"""Regression coverage for the public PTY send contract.

The public HTTP/socket send surface is intentionally fire-and-forget: callers
get only ``{"ok": true}``. The internal verified injector may still observe
echo/submit for agent delivery, but those fields are not response contract.

What a caller can assert now:
  1. the response contains only ``ok=true``;
  2. execution is observable through pane output after sending with Enter;
  3. ``enter=False`` does not execute until a later Enter is sent.

The size cases keep the old boundary payload lengths, but prove execution with
split markers so the assertion cannot be satisfied by terminal echo of the
typed command itself.
"""
from __future__ import annotations

import string
import time
import uuid

import pytest

from helpers import CodeTermAPI, shell_cmd


def _alpha(n: int) -> str:
    """Return a deterministic-per-call alphanumeric string of length n.
    Seeded by uuid so each test gets a unique marker; no shell metacharacters.
    """
    pool = string.ascii_lowercase + string.digits
    seed = uuid.uuid4().hex
    out = []
    while len(out) < n:
        for ch in seed:
            if ch in pool:
                out.append(ch)
                if len(out) == n:
                    break
        seed = uuid.uuid4().hex
    return "".join(out)


SHORT_LEN = 40
MEDIUM_LEN = 700
LONG_LEN = 5000


def _split_marker(prefix: str) -> tuple[str, str, str]:
    marker = prefix + _alpha(12)
    split = len(marker) // 2
    return marker, marker[:split], marker[split:]


def _assign_and_echo_marker(payload: str, marker_prefix: str) -> tuple[str, str]:
    marker, left, right = _split_marker(marker_prefix)
    unix = f"x='{payload}'; echo '{left}''{right}'"
    win = f"$x = '{payload}'; Write-Output ('{left}' + '{right}')"
    return shell_cmd(unix, win), marker


def _short_text() -> tuple[str, str]:
    marker, left, right = _split_marker("short_ok_")
    return shell_cmd(f"echo '{left}''{right}'", f"Write-Output ('{left}' + '{right}')"), marker


def _medium_text() -> tuple[str, str]:
    payload = _alpha(MEDIUM_LEN)
    return _assign_and_echo_marker(payload, "medium_ok_")


def _long_single_line() -> tuple[str, str]:
    payload = _alpha(LONG_LEN)
    return _assign_and_echo_marker(payload, "single_ok_")


def _long_multiline() -> tuple[str, str]:
    """Multi-line via printf with embedded \\n. Renders ~70 lines of ~70 chars
    each — guaranteed to scroll out of any reasonable visible-screen window."""
    chunk = "MULTI_" + _alpha(60)
    body = "".join(f"{chunk}_{i:03d}\\n" for i in range(70))
    return _assign_and_echo_marker(body, "multi_ok_")


CASES = [
    ("short40", _short_text),
    ("medium700", _medium_text),
    ("single5k", _long_single_line),
    ("multi5k", _long_multiline),
]


@pytest.mark.parametrize("label,builder", CASES, ids=[c[0] for c in CASES])
def test_inject_verify_contract(api: CodeTermAPI, fresh_pane: str, label, builder) -> None:
    text, marker = builder()

    resp = api.send_text(fresh_pane, text=text, enter=True)

    # 1. Public send is fire-and-forget: no verified/echo/submit response fields.
    assert resp == {"ok": True}, f"{label}: public send response changed: {resp}"

    # 2. The Enter byte actually landed in the PTY. The marker is split in the
    #    typed command and exists only in real command output.
    deadline = time.monotonic() + 6.0
    output = ""
    while time.monotonic() < deadline:
        data = api.get_output(fresh_pane, strip_ansi_flag=True)
        output = data.get("output", "")
        if marker in output:
            break
        time.sleep(0.3)

    assert marker in output, (
        f"{label}: split marker {marker!r} missing from command output; "
        f"tail={output[-400:]!r}"
    )
    tail = output.rsplit(marker, 1)[-1]
    assert any(s in tail for s in ("$", ">", "#", "%")), (
        f"{label}: no fresh prompt after marker — Enter byte may not have submitted; "
        f"tail={tail[-200:]!r}"
    )


def test_short_payload_execution_is_observable(api: CodeTermAPI, fresh_pane: str) -> None:
    """A clean short send returns the public ok contract and is observable in output."""
    marker, left, right = _split_marker("verifyok_")
    text = shell_cmd(f"echo '{left}''{right}'", f"Write-Output ('{left}' + '{right}')")
    resp = api.send_text(fresh_pane, text=text, enter=True)

    assert resp == {"ok": True}, resp
    output = api.wait_for_output(fresh_pane, pattern=marker, timeout=6)
    assert marker in output


def test_send_without_enter_still_ok(api: CodeTermAPI, fresh_pane: str) -> None:
    """``enter=False`` accepts the paste but does not execute until Enter arrives."""
    marker, left, right = _split_marker("noenter_")
    text = shell_cmd(f"echo '{left}''{right}'", f"Write-Output ('{left}' + '{right}')")
    resp = api.send_text(fresh_pane, text=text, enter=False)
    assert resp == {"ok": True}, resp

    deadline = time.monotonic() + 2.0
    output = ""
    while time.monotonic() < deadline:
        output = api.get_output(fresh_pane, strip_ansi_flag=True).get("output", "")
        assert marker not in output, (
            f"enter=false unexpectedly executed command; marker={marker!r} "
            f"tail={output[-400:]!r}"
        )
        time.sleep(0.2)

    submit_resp = api.send_text(fresh_pane, text="", enter=True)
    assert submit_resp == {"ok": True}, submit_resp
    output = api.wait_for_output(fresh_pane, pattern=marker, timeout=6)
    assert marker in output
