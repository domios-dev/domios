"""Real-world hierarchy scenarios — agents talk to each other, do real tasks, report back.

Each test spawns a fresh workspace with agents, waits for completion, verifies reports arrived.
These are slower (~30-60s each) but test the full agent communication pipeline.
"""

import sys
import time
import pytest
from helpers import CodeTermAPI, IS_WINDOWS, tmp_path, unique_name


TOKEN_TIMEOUT = 120  # max seconds to wait for agent work


def _create_workspace(api: CodeTermAPI, name: str) -> tuple[str, str]:
    """Create workspace + orchestrator pane, return (ws_id, orch_pane_id)."""
    ws = api.create_workspace(unique_name(f"e2e-ws-{name}"))
    ws_id = ws["id"]
    tab = api.create_tab(ws_id)
    orch = tab["pane_id"]
    api.post(f"/api/panes/{orch}/role", json={"role": "Orchestrator"})
    api.wait_for_pty(orch, timeout=15)
    return ws_id, orch


def _wait_for_report(api, parent_pane, marker, timeout=TOKEN_TIMEOUT):
    """Wait for a report containing marker to arrive at parent."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            output = api.get_output(parent_pane, strip_ansi_flag=True)
            if marker in output.get("output", ""):
                return True
            reports = api.get(f"/api/panes/{parent_pane}/reports")
            for r in reports:
                if marker in r.get("message", ""):
                    return True
        except Exception:
            pass
        time.sleep(3)
    return False


def _cleanup_ws(api, ws_id):
    try:
        workspaces = api.list_workspaces()
        if len(workspaces) > 1:
            api.close_workspace(ws_id)
    except Exception:
        pass


# ── Scenario 1: Simple worker reports to orchestrator ──────────────────

def test_scenario_single_worker(api: CodeTermAPI):
    """One worker does a task and reports back."""
    ws_id, orch = _create_workspace(api, "single-worker")
    try:
        marker = unique_name("S1")
        worker = api.post("/api/agents/spawn", json={
            "agent": "claude",
            "title": "Worker",
            "role": "Worker",
            "parent_pane_id": orch,
            "task": f'Run: echo {marker}. Then report your result.',
        })
        found = _wait_for_report(api, orch, marker, timeout=90)
        assert found, f"Report with {marker} not received"
    finally:
        _cleanup_ws(api, ws_id)


# ── Scenario 2: Two workers report independently ──────────────────────

def test_scenario_two_independent_workers(api: CodeTermAPI):
    """Two workers run in parallel, both report to same orchestrator."""
    ws_id, orch = _create_workspace(api, "two-workers")
    try:
        m1 = unique_name("W1")
        m2 = unique_name("W2")
        api.post("/api/agents/spawn", json={
            "agent": "claude", "title": "Alpha", "role": "Worker",
            "parent_pane_id": orch,
            "task": f'Run: echo {m1}. Report the result.',
        })
        api.post("/api/agents/spawn", json={
            "agent": "claude", "title": "Beta", "role": "Worker",
            "parent_pane_id": orch,
            "task": f'Run: echo {m2}. Report the result.',
        })
        f1 = _wait_for_report(api, orch, m1, timeout=90)
        f2 = _wait_for_report(api, orch, m2, timeout=90)
        assert f1, f"Report from Alpha ({m1}) not received"
        assert f2, f"Report from Beta ({m2}) not received"
    finally:
        _cleanup_ws(api, ws_id)


# ── Scenario 4: Worker does file operations ───────────────────────────

def test_scenario_worker_file_ops(api: CodeTermAPI):
    """Worker creates a temp file, reads it, reports content."""
    ws_id, orch = _create_workspace(api, "file-ops")
    try:
        marker = unique_name("FILE")
        fpath = tmp_path(f"{marker}.txt")
        api.post("/api/agents/spawn", json={
            "agent": "claude", "title": "File Worker", "role": "Worker",
            "parent_pane_id": orch,
            "task": f'Create file {fpath} with content "{marker}_CONTENT". Then read it back and report the content.',
        })
        found = _wait_for_report(api, orch, f"{marker}_CONTENT", timeout=90)
        assert found, f"File content report not received"
    finally:
        _cleanup_ws(api, ws_id)


# ── Scenario 5: Worker runs multi-step pipeline ──────────────────────

def test_scenario_pipeline(api: CodeTermAPI):
    """Worker runs a multi-step pipeline and reports."""
    ws_id, orch = _create_workspace(api, "pipeline")
    try:
        marker = unique_name("PIPE")
        fpath = tmp_path(f"{marker}_pipe.txt")
        api.post("/api/agents/spawn", json={
            "agent": "claude", "title": "Pipeline Worker", "role": "Worker",
            "parent_pane_id": orch,
            "task": (
                f'Write two lines to {fpath}:\n'
                f'  Line 1: "step1_{marker}"\n'
                f'  Line 2: "step2_{marker}"\n'
                f'Then count the lines in the file and report how many lines it has.'
            ),
        })
        found = _wait_for_report(api, orch, "2", timeout=90)
        assert found, "Pipeline worker did not report line count"
    finally:
        _cleanup_ws(api, ws_id)


# ── Scenario 6: Cascade close — kill boss kills everyone ──────────────

def test_scenario_cascade_close(api: CodeTermAPI):
    """Spawn hierarchy, close boss, verify all children die."""
    ws_id, orch = _create_workspace(api, "cascade")
    try:
        boss = api.post("/api/agents/spawn", json={
            "agent": "claude", "title": "Boss", "role": "Orchestrator",
            "workspace_id": ws_id, "can_spawn": True,
            "task": "Spawn 2 workers using codeterm CLI. Worker-1: title W1, role Worker, task 'wait'. Worker-2: title W2, role Worker, task 'wait'. Use your own pane ID as parent. Then just wait.",
        })
        boss_id = boss["pane_id"]
        api.wait_for_pty(boss_id, timeout=20)
        time.sleep(15)  # Let boss spawn workers

        # Count panes
        panes_before = len(api.list_panes())
        assert panes_before >= 3, f"Expected at least 3 panes, got {panes_before}"

        # Kill boss
        api.close_pane(boss_id)
        time.sleep(3)

        # Workers should be gone
        panes_after = api.list_panes()
        boss_children = [p for p in panes_after if p.get("parent_pane_id") == boss_id]
        assert len(boss_children) == 0, f"Boss children still alive: {boss_children}"
    finally:
        _cleanup_ws(api, ws_id)


# ── Scenario 7: Cron spawns agent that reports to creator ─────────────

def test_scenario_cron_report(api: CodeTermAPI):
    """Cron fires, agent reports to creator pane."""
    ws_id, orch = _create_workspace(api, "cron-rpt")
    try:
        marker = unique_name("CRON")
        # Create cron with creator_pane_id (via REST — cron.create doesn't go through CLI here)
        cron = api.post("/api/crons", json={
            "name": f"Test {marker}",
            "schedule": "0 0 1 1 *",  # Never fires naturally
            "task": f'Run: echo {marker}. Report the result to your creator.',
            "workspace_id": ws_id,
            "creator_pane_id": orch,
        })
        cron_id = cron["id"]

        # Trigger manually
        api.post(f"/api/crons/{cron_id}/trigger")

        # Wait for report at creator pane
        found = _wait_for_report(api, orch, marker, timeout=90)
        # Clean up cron
        try:
            api.delete(f"/api/crons/{cron_id}")
        except Exception:
            pass
        assert found, f"Cron agent report with {marker} not received"
    finally:
        _cleanup_ws(api, ws_id)


# ── Scenario 8: Worker with custom cwd ────────────────────────────────

def test_scenario_custom_cwd(api: CodeTermAPI):
    """Worker starts in temp directory, verifies cwd, reports."""
    import tempfile
    temp_dir = tempfile.gettempdir()
    ws_id, orch = _create_workspace(api, "cwd")
    try:
        marker = unique_name("CWD")
        api.post("/api/agents/spawn", json={
            "agent": "claude", "title": "CWD Worker", "role": "Worker",
            "parent_pane_id": orch, "cwd": temp_dir,
            "task": f'Check your current directory. If it contains "tmp" or "Temp", report "{marker}_OK". Otherwise report "{marker}_FAIL".',
        })
        found = _wait_for_report(api, orch, f"{marker}_OK", timeout=90)
        assert found, f"CWD worker did not confirm {temp_dir}"
    finally:
        _cleanup_ws(api, ws_id)


# ── Scenario 11: Worker error handling ────────────────────────────────

def test_scenario_worker_error(api: CodeTermAPI):
    """Worker runs a failing command, reports the error."""
    ws_id, orch = _create_workspace(api, "error")
    try:
        marker = unique_name("ERR")
        api.post("/api/agents/spawn", json={
            "agent": "claude", "title": "Error Worker", "role": "Worker",
            "parent_pane_id": orch,
            "task": f'Try to list a nonexistent directory (e.g. /nonexistent_path_12345 or C:\\nonexistent_path_12345). Report the error message including {marker}.',
        })
        found = _wait_for_report(api, orch, marker, timeout=90)
        assert found, "Error worker did not report"
    finally:
        _cleanup_ws(api, ws_id)


# ── Scenario 12: Batch spawn via API ──────────────────────────────────

def test_scenario_batch_spawn(api: CodeTermAPI):
    """Use batch API to spawn orchestrator + 2 workers in one call."""
    ws_id, _ = _create_workspace(api, "batch")
    try:
        marker = unique_name("BATCH")
        result = api.post("/api/batch", json={
            "commands": [
                {"method": "spawn", "id": "boss", "params": {
                    "title": "Batch Boss", "role": "Orchestrator",
                    "agent": "claude", "can_spawn": True,
                    "workspace_id": ws_id,
                    "task": f"Wait for reports from your workers. After all report, print {marker}_DONE.",
                }},
                {"method": "spawn", "params": {
                    "title": "BW-1", "role": "Worker",
                    "agent": "claude", "parent_pane_id": "$boss",
                    "task": f"echo {marker}_1. Report.",
                }},
                {"method": "spawn", "params": {
                    "title": "BW-2", "role": "Worker",
                    "agent": "claude", "parent_pane_id": "$boss",
                    "task": f"echo {marker}_2. Report.",
                }},
            ]
        })
        # result is array of spawn results
        assert len(result) == 3
        boss_pane = result[0].get("pane_id")
        assert boss_pane

        found = _wait_for_report(api, boss_pane, marker, timeout=TOKEN_TIMEOUT)
        assert found, f"Batch workers did not report to boss"
    finally:
        _cleanup_ws(api, ws_id)


# ── Scenario 13: Notification from worker ─────────────────────────────

def test_scenario_notification(api: CodeTermAPI):
    """Worker sends a notification, verify it appears."""
    ws_id, orch = _create_workspace(api, "notif")
    try:
        marker = unique_name("NOTIF")
        api.post("/api/notifications/clear")
        api.post("/api/agents/spawn", json={
            "agent": "claude", "title": "Notifier", "role": "Worker",
            "parent_pane_id": orch,
            "task": f'Run: codeterm notify "{marker}" --level info. Then report done.',
        })
        # Wait for notification
        deadline = time.monotonic() + 90
        found = False
        while time.monotonic() < deadline:
            notifs = api.get("/api/notifications")
            if any(marker in n.get("title", "") for n in notifs):
                found = True
                break
            time.sleep(3)
        assert found, f"Notification {marker} not found"
    finally:
        _cleanup_ws(api, ws_id)


# ── Scenario 14: Rapid fire — 5 workers at once ──────────────────────

def test_scenario_rapid_fire(api: CodeTermAPI):
    """Spawn 5 workers simultaneously, all report back."""
    ws_id, orch = _create_workspace(api, "rapid")
    try:
        markers = [unique_name(f"RF{i}") for i in range(5)]
        for i, m in enumerate(markers):
            api.post("/api/agents/spawn", json={
                "agent": "claude", "title": f"Rapid-{i}", "role": "Worker",
                "parent_pane_id": orch,
                "task": f"echo {m}. Report the result.",
            })

        results = []
        for m in markers:
            found = _wait_for_report(api, orch, m, timeout=TOKEN_TIMEOUT)
            results.append((m, found))

        failed = [m for m, f in results if not f]
        assert len(failed) == 0, f"Reports not received from: {failed}"
    finally:
        _cleanup_ws(api, ws_id)


# ── Scenario 15: Worker renames its own tab ───────────────────────────

def test_scenario_self_rename(api: CodeTermAPI):
    """Worker renames its tab, verify the rename sticks."""
    ws_id, orch = _create_workspace(api, "rename")
    try:
        marker = unique_name("REN")
        worker = api.post("/api/agents/spawn", json={
            "agent": "claude", "title": "Renamer", "role": "Worker",
            "parent_pane_id": orch,
            "task": f'Rename your tab: codeterm pane rename "{marker}_TAB". Then report done.',
        })
        worker_id = worker["pane_id"]
        # Wait for report
        _wait_for_report(api, orch, "done", timeout=90)
        # Check rename stuck
        panes = api.list_panes()
        worker_pane = next((p for p in panes if p["id"] == worker_id), None)
        if worker_pane:
            assert marker in worker_pane.get("title", ""), f"Tab not renamed: {worker_pane.get('title')}"
    finally:
        _cleanup_ws(api, ws_id)
