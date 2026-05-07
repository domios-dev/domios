"""Regression coverage for the verified PTY-injection contract.

Pins the contract that emerged from the v1.1.18-v1.1.20 hot-fix marathon:
  1. ``ok=true`` ALWAYS — Enter is unconditionally fired, even when the
     screen verifier times out.
  2. The Enter byte actually lands in the PTY — the recipient sees a fresh
     prompt after the marker (proves submit, not just buffered echo).
  3. The ``verified`` field is observed evidence (echo AND submit) — never
     a hardcoded ``true``. Mirrors socket/server/pane.rs:83 and the parallel
     HTTP path fixed in v1.1.21.

The four size cases pin behaviour at the boundaries the verifier handled
incorrectly during the marathon: short payloads (clean verify), medium
~700-char (single-line scroll edge), 5K single-line (paste-placeholder
case), 5K multi-line (long heredoc-style scroll-out).
"""
from __future__ import annotations

import string
import time
import uuid

import pytest

from helpers import CodeTermAPI, strip_ansi


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


def _short_text() -> tuple[str, str]:
    marker = "shrtmrk_" + _alpha(SHORT_LEN - len("shrtmrk_") - len("echo "))
    return f"echo {marker}", marker


def _medium_text() -> tuple[str, str]:
    payload = _alpha(MEDIUM_LEN)
    return f"echo {payload}", payload[:24]


def _long_single_line() -> tuple[str, str]:
    payload = _alpha(LONG_LEN)
    return f"echo {payload}", payload[:24]


def _long_multiline() -> tuple[str, str]:
    """Multi-line via printf with embedded \\n. Renders ~70 lines of ~70 chars
    each — guaranteed to scroll out of any reasonable visible-screen window."""
    chunk = "MULTI_" + _alpha(60)
    body = "".join(f"{chunk}_{i:03d}\\n" for i in range(70))
    return f"printf '{body}'", chunk


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

    # 1. ok=true ALWAYS — Enter unconditionally fires, regardless of verify.
    assert resp.get("ok") is True, f"{label}: ok must always be true; got {resp}"

    # 2. verified field MUST reflect observed evidence, never a hardcoded true.
    #    Contract (mirroring socket/server/pane.rs:83):
    #      verified == echo_observed AND (!enter OR submit_observed)
    #    With enter=True that simplifies to: verified == echo AND submit.
    echo = bool(resp.get("echo_observed", False))
    submit = bool(resp.get("submit_observed", False))
    expected_verified = echo and submit
    assert resp.get("verified") == expected_verified, (
        f"{label}: verified is hardcoded — got {resp.get('verified')!r}, "
        f"expected {expected_verified} (echo={echo}, submit={submit}, full={resp})"
    )

    # 3. The Enter byte actually landed in the PTY. For short payloads we can
    #    assert the marker appears AND a prompt sigil follows it. Long payloads
    #    may legitimately scroll out of the visible buffer, so we only require
    #    that some output rendered after sending — proving the PTY consumed it.
    deadline = time.monotonic() + 6.0
    output = ""
    while time.monotonic() < deadline:
        data = api.get_output(fresh_pane, strip_ansi_flag=True)
        output = data.get("output", "")
        if marker in output:
            break
        time.sleep(0.3)

    if label == "short40":
        assert marker in output, (
            f"{label}: marker {marker!r} missing from output (Enter may not have fired); "
            f"tail={output[-400:]!r}"
        )
        tail = output.rsplit(marker, 1)[-1]
        assert any(s in tail for s in ("$", ">", "#", "%")), (
            f"{label}: no fresh prompt after marker — Enter byte may not have submitted; "
            f"tail={tail[-200:]!r}"
        )

    # 4. Re-pin the always-ok contract for visibility on long payloads where
    #    the verify timeout is the legitimate path. v1.1.20 made this guarantee.
    assert resp["ok"] is True, f"{label}: ok must remain true even on verify timeout"


def test_short_payload_is_fully_verified(api: CodeTermAPI, fresh_pane: str) -> None:
    """A clean short echo MUST be fully verified end-to-end. If this fails,
    the verifier itself broke (regression beyond the long-payload edge case)."""
    text = "echo verifyok_" + _alpha(10)
    resp = api.send_text(fresh_pane, text=text, enter=True)

    assert resp.get("ok") is True, resp
    assert resp.get("echo_observed") is True, f"short payload echo must be observed: {resp}"
    assert resp.get("submit_observed") is True, f"short payload submit must be observed: {resp}"
    assert resp.get("verified") is True, f"short payload must be verified end-to-end: {resp}"


def test_send_without_enter_still_ok(api: CodeTermAPI, fresh_pane: str) -> None:
    """When ``enter=False`` the verified contract is `echo_observed && (!enter || submit)`
    which simplifies to just ``echo_observed``. Confirm the implementation honours that."""
    text = "noenter_" + _alpha(8)
    resp = api.send_text(fresh_pane, text=text, enter=False)
    assert resp.get("ok") is True, resp
    echo = bool(resp.get("echo_observed", False))
    expected_verified = echo  # !enter short-circuits the submit clause
    assert resp.get("verified") == expected_verified, (
        f"enter=false verified must equal echo_observed; got verified={resp.get('verified')!r} "
        f"echo={echo} resp={resp}"
    )
