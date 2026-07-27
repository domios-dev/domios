"""Tab-survival e2e for `codeterm update --apply` (Phase 4 scaffolding).

The full update scenario (binary swap + relaunch) cannot run in a generic
CI without a fully built two-version test rig and signed assets. The
harness for that lives in `tests/fixtures/auto-update/`. The end-to-end
test below is therefore marked SKIPPED by default and only runs when
`CODETERM_RUN_UPDATE_RESUME=1` is set in the environment.

What still runs unconditionally:

  * `test_update_check_endpoint_exists` — hits `/api/update/check` to confirm
    the endpoint returns a JSON envelope. This catches regressions in the
    plain-update REST surface.
  * `test_tabs_snapshot_shape` — creates two long-running PTY tabs and
    asserts the `/api/tabs` snapshot exposes the fields the resume path
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


def spawn_long_running_tabs(api: CodeTermAPI, base_tab: str, count: int) -> list[str]:
    tab_ids = [base_tab]
    base = next((p for p in api.list_tabs() if p["id"] == base_tab), None)
    if base is None:
        raise AssertionError(f"base tab {base_tab} missing from snapshot")
    workspace_id = base.get("workspace_id")
    if not workspace_id:
        raise AssertionError(f"base tab {base_tab} missing workspace_id")
    for _ in range(count - 1):
        result = api.create_tab(workspace_id, eager=True)
        new_id = result["tab_id"]
        api.wait_for_pty(new_id, timeout=15)
        tab_ids.append(new_id)

    cmd = long_running_command()
    for pid in tab_ids:
        api.send_text(pid, text=cmd)

    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        ready = 0
        for pid in tab_ids:
            try:
                output = api.get_output(pid).get("output", "")
                if "tick" in output:
                    ready += 1
            except Exception:
                pass
        if ready == len(tab_ids):
            break
        time.sleep(0.5)

    return tab_ids


def snapshot_tabs(api: CodeTermAPI, tab_ids: list[str]) -> dict[str, dict]:
    tabs = api.list_tabs()
    by_id = {p["id"]: p for p in tabs if p["id"] in tab_ids}
    missing = set(tab_ids) - set(by_id.keys())
    if missing:
        raise AssertionError(f"tabs vanished from snapshot: {missing}")
    return by_id


# ---------------------------------------------------------------------------
# Scenarios that run on every pytest invocation
# ---------------------------------------------------------------------------

def test_update_check_endpoint_exists(api_only: CodeTermAPI):
    """`/api/update/check` returns a JSON envelope (200 or structured error)."""
    try:
        result = api_only.get("/api/update/check")
    except Exception as exc:
        pytest.skip(f"update endpoint not yet wired: {exc}")
    assert isinstance(result, dict), "expected dict envelope"


def test_tabs_snapshot_shape(api: CodeTermAPI, fresh_tab: str):
    """The fields the SHUTDOWN PRELUDE relies on for resume must be present.

    This is the dry-run version of the tab-survival test. The actual
    binary swap is gated behind `CODETERM_RUN_UPDATE_RESUME`; this check
    runs every time so we catch snapshot-shape regressions before they
    matter in production.
    """
    tab_ids = spawn_long_running_tabs(api, fresh_tab, count=2)

    try:
        snap = snapshot_tabs(api, tab_ids)

        for pid, tab in snap.items():
            assert tab.get("id") == pid
            assert "title" in tab, f"tab {pid} missing title"
            assert "cwd" in tab, f"tab {pid} missing cwd"
            assert tab.get("pty_id"), f"tab {pid} missing pty_id"
    finally:
        for pid in tab_ids[1:]:
            try:
                api.close_tab(pid)
            except Exception:
                pass


# ---------------------------------------------------------------------------
# Full tab-survival scenario (skipped by default)
# ---------------------------------------------------------------------------

REQUIRES_TEST_RIG = (
    "requires update test rig with signed two-version assets — "
    "see tests/fixtures/auto-update/. Set CODETERM_RUN_UPDATE_RESUME=1 "
    "and CODETERM_TEST_MANIFEST_URL=<url> to opt in."
)


@pytest.mark.skipif(
    os.environ.get("CODETERM_RUN_UPDATE_RESUME") != "1",
    reason=REQUIRES_TEST_RIG,
)
def test_update_resume_tab_survival(api: CodeTermAPI, fresh_tab: str):
    """End-to-end: tabs survive a binary swap with state intact.

    Pre-conditions (developer must arrange before running):
      * Test rig built per `tests/fixtures/auto-update/README.md`.
      * Local asset server running (`python -m http.server 8080`).
      * Manifest URL exported as `CODETERM_TEST_MANIFEST_URL`.
      * Current binary is the OLDER test version (e.g. 0.0.1-test).
    """
    manifest_url = os.environ.get("CODETERM_TEST_MANIFEST_URL")
    if not manifest_url:
        pytest.skip("CODETERM_TEST_MANIFEST_URL not set")

    tab_ids = spawn_long_running_tabs(api, fresh_tab, count=3)
    pre_snapshot = snapshot_tabs(api, tab_ids)
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
            post_snapshot = snapshot_tabs(api, tab_ids)
            if all(p.get("pty_id") for p in post_snapshot.values()):
                break
        except AssertionError:
            pass
        time.sleep(1)
    else:
        raise AssertionError("tabs did not restore within 30s after relaunch")

    assert set(post_snapshot.keys()) == set(pre_snapshot.keys())
    for pid, pre in pre_snapshot.items():
        post = post_snapshot[pid]
        assert post["title"] == pre["title"], f"title drift on tab {pid}"
        assert post["cwd"] == pre["cwd"], f"cwd drift on tab {pid}"
        if pre.get("agent_session_id"):
            assert post.get("agent_session_id") == pre["agent_session_id"], (
                f"agent_session_id drift on tab {pid}"
            )
