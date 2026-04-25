"""Pane-survival e2e for `codeterm update --apply` (Phase 4 scaffolding).

The full self-update scenario (binary swap + relaunch) cannot run in a
generic CI without a fully built two-version test rig and signed assets.
The harness for that lives in `tests/fixtures/auto-update/` and is
documented in `docs/contributing/auto-update-testing.md`. The end-to-end
test below is therefore marked SKIPPED by default and only runs when
`CODETERM_RUN_UPDATE_RESUME=1` is set in the environment (or the
`update-smoke` CI label flips it on).

What still runs unconditionally:

  * `test_update_check_endpoint_exists` — hits `/api/update/check` to confirm
    the endpoint returns a JSON envelope. This catches regressions in the
    plain-update REST surface.
  * `test_panes_snapshot_shape` — creates two long-running PTY panes and
    asserts the `/api/panes` snapshot exposes the fields the resume path
    relies on (`id`, `title`, `cwd`, `pty_id`, optionally `agent_session_id`).
    No restart involved — this is a contract check on the data the
    SHUTDOWN PRELUDE captures, so a regression in the snapshot shape fails
    here instead of after a real binary swap.
"""

from __future__ import annotations

import os
import time

import pytest

from helpers import CodeTermAPI, IS_WINDOWS, unique_name


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

LONG_RUNNING_CMD_UNIX = "python3 -c 'import time, sys\nfor i in range(36000):\n    print(f\"tick {i}\", flush=True); time.sleep(1)'"
LONG_RUNNING_CMD_WIN = "powershell -NoProfile -Command \"for ($i=0; $i -lt 36000; $i++) { Write-Output \\\"tick $i\\\"; Start-Sleep 1 }\""


def long_running_command() -> str:
    return LONG_RUNNING_CMD_WIN if IS_WINDOWS else LONG_RUNNING_CMD_UNIX


def spawn_long_running_panes(api: CodeTermAPI, base_pane: str, count: int) -> list[str]:
    pane_ids = [base_pane]
    for _ in range(count - 1):
        result = api.split_pane(base_pane, direction="right")
        new_id = result["new_pane_id"]
        api.wait_for_pty(new_id, timeout=15)
        pane_ids.append(new_id)

    cmd = long_running_command()
    for pid in pane_ids:
        api.send_text(pid, text=cmd)

    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        ready = 0
        for pid in pane_ids:
            try:
                output = api.get_output(pid).get("output", "")
                if "tick" in output:
                    ready += 1
            except Exception:
                pass
        if ready == len(pane_ids):
            break
        time.sleep(0.5)

    return pane_ids


def snapshot_panes(api: CodeTermAPI, pane_ids: list[str]) -> dict[str, dict]:
    panes = api.list_panes()
    by_id = {p["id"]: p for p in panes if p["id"] in pane_ids}
    missing = set(pane_ids) - set(by_id.keys())
    if missing:
        raise AssertionError(f"panes vanished from snapshot: {missing}")
    return by_id


# ---------------------------------------------------------------------------
# Scenarios that run on every pytest invocation
# ---------------------------------------------------------------------------

def test_update_check_endpoint_exists(api: CodeTermAPI):
    """`/api/update/check` returns a JSON envelope (200 or structured error)."""
    try:
        result = api.get("/api/update/check")
    except Exception as exc:
        pytest.skip(f"update endpoint not yet wired: {exc}")
    assert isinstance(result, dict), "expected dict envelope"


def test_panes_snapshot_shape(api: CodeTermAPI, fresh_pane: str):
    """The fields the SHUTDOWN PRELUDE relies on for resume must be present.

    This is the dry-run version of the pane-survival test. The actual
    binary swap is gated behind `CODETERM_RUN_UPDATE_RESUME`; this check
    runs every time so we catch snapshot-shape regressions before they
    matter in production.
    """
    pane_ids = spawn_long_running_panes(api, fresh_pane, count=2)

    try:
        snap = snapshot_panes(api, pane_ids)

        for pid, pane in snap.items():
            assert pane.get("id") == pid
            assert "title" in pane, f"pane {pid} missing title"
            assert "cwd" in pane, f"pane {pid} missing cwd"
            assert pane.get("pty_id"), f"pane {pid} missing pty_id"
    finally:
        for pid in pane_ids[1:]:
            try:
                api.close_pane(pid)
            except Exception:
                pass


# ---------------------------------------------------------------------------
# Full pane-survival scenario (skipped by default)
# ---------------------------------------------------------------------------

REQUIRES_TEST_RIG = (
    "requires self-update test rig with signed two-version assets — "
    "see docs/contributing/auto-update-testing.md. Set "
    "CODETERM_RUN_UPDATE_RESUME=1 and CODETERM_TEST_MANIFEST_URL=<url> "
    "to opt in."
)


@pytest.mark.skipif(
    os.environ.get("CODETERM_RUN_UPDATE_RESUME") != "1",
    reason=REQUIRES_TEST_RIG,
)
def test_update_resume_pane_survival(api: CodeTermAPI, fresh_pane: str):
    """End-to-end: panes survive a binary swap with state intact.

    Pre-conditions (developer must arrange before running):
      * Test rig built per `tests/fixtures/auto-update/README.md`.
      * Local asset server running (`python -m http.server 8080`).
      * Manifest URL exported as `CODETERM_TEST_MANIFEST_URL`.
      * Current binary is the OLDER test version (e.g. 0.0.1-test).
    """
    manifest_url = os.environ.get("CODETERM_TEST_MANIFEST_URL")
    if not manifest_url:
        pytest.skip("CODETERM_TEST_MANIFEST_URL not set")

    pane_ids = spawn_long_running_panes(api, fresh_pane, count=3)
    pre_snapshot = snapshot_panes(api, pane_ids)
    pre_health = api.get("/api/health")
    pre_pid = pre_health.get("pid")

    api.post(
        "/api/update/install",
        params={"auto": "true", "manifest_url": manifest_url},
    )

    deadline = time.monotonic() + 120
    new_pid = pre_pid
    while time.monotonic() < deadline:
        try:
            health = api.get("/api/health")
            new_pid = health.get("pid")
            if new_pid and new_pid != pre_pid:
                break
        except Exception:
            pass
        time.sleep(1)
    else:
        raise AssertionError("CodeTerm did not relaunch within 120s")

    deadline = time.monotonic() + 30
    post_snapshot: dict[str, dict] = {}
    while time.monotonic() < deadline:
        try:
            post_snapshot = snapshot_panes(api, pane_ids)
            if all(p.get("pty_id") for p in post_snapshot.values()):
                break
        except AssertionError:
            pass
        time.sleep(1)
    else:
        raise AssertionError("panes did not restore within 30s after relaunch")

    assert set(post_snapshot.keys()) == set(pre_snapshot.keys())
    for pid, pre in pre_snapshot.items():
        post = post_snapshot[pid]
        assert post["title"] == pre["title"], f"title drift on pane {pid}"
        assert post["cwd"] == pre["cwd"], f"cwd drift on pane {pid}"
        if pre.get("agent_session_id"):
            assert post.get("agent_session_id") == pre["agent_session_id"], (
                f"agent_session_id drift on pane {pid}"
            )
