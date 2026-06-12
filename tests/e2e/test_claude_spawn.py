"""Claude Code spawn tests — the critical pipeline.

Tests the full lifecycle: spawn via API → shell prompt detection →
Claude launch → readiness detection → task delivery → output verification.

These tests spawn REAL Claude Code instances (with --dangerously-skip-permissions).
They are slower than other e2e tests (~30-60s each).
"""

import time

import pytest
from helpers import CodeTermAPI, unique_name

pytestmark = pytest.mark.llm


# Marker we can search for in output to confirm Claude is alive
CLAUDE_READY_MARKERS = ["shift+tab", "esc to interrupt", "hold Space", "ctrl+g"]


def _wait_for_claude_ready(api: CodeTermAPI, pane_id: str, timeout: float = 60) -> str:
    """Wait until Claude Code is interactive (shows prompt markers on screen)."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            screen = api.get_screen(pane_id)
            for marker in CLAUDE_READY_MARKERS:
                if marker in screen:
                    return screen
        except Exception:
            pass
        time.sleep(2)
    raise TimeoutError(f"Claude not ready in pane {pane_id} within {timeout}s")


def _spawn_claude(api: CodeTermAPI, **kwargs) -> str:
    """Spawn Claude via API, wait for PTY, return pane_id."""
    defaults = {
        "agent": "claude",
        "title": unique_name("claude-test"),
    }
    defaults.update(kwargs)
    result = api.post("/api/agents/spawn", json=defaults)
    pane_id = result["pane_id"]

    # Wait for PTY to be linked
    assert api.wait_for_pty(pane_id, timeout=20), f"PTY not linked for {pane_id}"
    return pane_id


@pytest.fixture()
def claude_ws(api: CodeTermAPI):
    """Create a workspace with an Orchestrator pane, yield (ws_id, orch_pane_id), cleanup."""
    ws = api.create_workspace(unique_name("e2e-ws"))
    ws_id = ws["id"]
    tab = api.create_tab(ws_id)
    orch_pane = tab["pane_id"]
    # Set as Orchestrator so we can spawn children under it
    api.post(f"/api/panes/{orch_pane}/role", json={"role": "Orchestrator"})
    api.wait_for_pty(orch_pane, timeout=15)
    yield ws_id, orch_pane
    # Cleanup: close workspace but never the last one (would kill the app)
    try:
        workspaces = api.list_workspaces()
        if len(workspaces) > 1:
            api.close_workspace(ws_id)
    except Exception:
        pass


def test_spawn_claude_basic(api: CodeTermAPI, claude_ws):
    """Spawn Claude with no task — verify it starts and shows interactive prompt."""
    ws_id, _ = claude_ws
    pane_id = _spawn_claude(api, workspace_id=ws_id)

    try:
        screen = _wait_for_claude_ready(api, pane_id, timeout=90)
        # Claude should show its interactive UI
        assert any(m in screen for m in CLAUDE_READY_MARKERS), \
            f"Claude not showing interactive markers. Screen: {screen[-200:]}"
    finally:
        api.close_pane(pane_id)


def test_spawn_claude_with_task(api: CodeTermAPI, claude_ws):
    """Spawn Claude with a simple task — verify task is delivered and executed."""
    ws_id, _ = claude_ws
    task_marker = unique_name("TASK")
    task = f'echo "{task_marker}" and then just say exactly: DONE_{task_marker}'

    pane_id = _spawn_claude(api, workspace_id=ws_id, task=task)

    try:
        # Wait for Claude to process the task — look for our marker in screen/output
        deadline = time.monotonic() + 90
        found = False
        while time.monotonic() < deadline:
            try:
                screen = api.get_screen(pane_id)
                if task_marker in screen:
                    found = True
                    break
            except Exception:
                pass
            time.sleep(2)

        assert found, f"Task marker '{task_marker}' not found in Claude output within 90s"
    finally:
        api.close_pane(pane_id)


def test_spawn_claude_as_worker(api: CodeTermAPI, claude_ws):
    """Spawn Claude as Worker under an Orchestrator — verify role and hierarchy."""
    _, orch_pane = claude_ws
    pane_id = _spawn_claude(
        api,
        role="Worker",
        parent_pane_id=orch_pane,
        task="Say exactly: WORKER_READY",
    )

    try:
        # Verify role + parent linkage (hierarchy is derived from
        # parent_pane_id; the /children endpoint was removed).
        time.sleep(2)
        status = api.get(f"/api/panes/{pane_id}/status")
        assert status.get("role") == "Worker"
        assert status.get("parent_pane_id") == orch_pane
    finally:
        api.close_pane(pane_id)


def test_spawn_claude_with_cwd(api: CodeTermAPI, claude_ws):
    """Spawn Claude with custom cwd — verify it starts in the right directory."""
    ws_id, _ = claude_ws
    pane_id = _spawn_claude(api, workspace_id=ws_id, cwd="/tmp", task="Run: pwd")

    try:
        # Wait for Claude and look for /tmp in output
        deadline = time.monotonic() + 90
        found = False
        while time.monotonic() < deadline:
            try:
                screen = api.get_screen(pane_id)
                if "/tmp" in screen or "/private/tmp" in screen:
                    found = True
                    break
            except Exception:
                pass
            time.sleep(2)

        assert found, "Claude did not show /tmp in output"
    finally:
        api.close_pane(pane_id)


def test_spawn_multiple_claude_agents(api: CodeTermAPI, claude_ws):
    """Spawn 3 Claude workers in parallel — all should start independently."""
    _, orch_pane = claude_ws
    pane_ids = []

    for i in range(3):
        pid = _spawn_claude(
            api,
            title=f"Worker-{i}",
            role="Worker",
            parent_pane_id=orch_pane,
            task=f"Say exactly: MULTI_WORKER_{i}_OK",
        )
        pane_ids.append(pid)

    try:
        time.sleep(5)
        panes = api.list_panes()
        for pid in pane_ids:
            pane = next((p for p in panes if p["id"] == pid), None)
            assert pane is not None, f"Pane {pid} not found"
            assert pane.get("pty_id"), f"Pane {pid} has no PTY"
            # hierarchy is derived from parent_pane_id (the /children endpoint
            # was removed).
            assert pane.get("parent_pane_id") == orch_pane, \
                f"Worker {pid} not linked to orchestrator"
    finally:
        for pid in pane_ids:
            try:
                api.close_pane(pid)
            except Exception:
                pass


def test_spawn_claude_report_to_parent(api: CodeTermAPI, claude_ws):
    """Spawn Claude worker with task that includes reporting — verify report arrives."""
    _, orch_pane = claude_ws
    report_marker = unique_name("RPT")
    pane_id = _spawn_claude(
        api,
        role="Worker",
        parent_pane_id=orch_pane,
        task=f'Run this command: codeterm agent report "{report_marker}"',
    )

    try:
        deadline = time.monotonic() + 90
        found = False
        while time.monotonic() < deadline:
            try:
                output = api.get_output(orch_pane, strip_ansi_flag=True)
                if report_marker in output.get("output", ""):
                    found = True
                    break
                reports = api.get(f"/api/panes/{orch_pane}/reports")
                for r in reports:
                    if report_marker in r.get("message", ""):
                        found = True
                        break
                if found:
                    break
            except Exception:
                pass
            time.sleep(3)

        assert found, f"Report with marker '{report_marker}' not received by parent within 90s"
    finally:
        api.close_pane(pane_id)


def test_close_orchestrator_cascades(api: CodeTermAPI):
    """Closing orchestrator kills all children."""
    # Create a workspace with orchestrator
    ws = api.create_workspace(unique_name("e2e-ws"))
    ws_id = ws["id"]
    tab = api.create_tab(ws_id)
    orch_id = tab["pane_id"]
    api.post(f"/api/panes/{orch_id}/role", json={"role": "Orchestrator"})
    api.wait_for_pty(orch_id, timeout=15)

    # Spawn workers
    w1 = _spawn_claude(api, role="Worker", parent_pane_id=orch_id, task="Wait")
    w2 = _spawn_claude(api, role="Worker", parent_pane_id=orch_id, task="Wait")

    time.sleep(2)

    # Close orchestrator — should cascade
    api.close_pane(orch_id)
    time.sleep(2)

    # Workers should be gone
    panes = api.list_panes()
    pane_ids = [p["id"] for p in panes]
    assert w1 not in pane_ids, "Worker 1 should be killed by cascade"
    assert w2 not in pane_ids, "Worker 2 should be killed by cascade"

    # Cleanup workspace
    try:
        api.close_workspace(ws_id)
    except Exception:
        pass
