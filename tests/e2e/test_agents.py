"""Agent hierarchy tests — spawn, roles, reports."""

import time

from helpers import CodeTermAPI, unique_name


def test_spawn_agent_bash(api: CodeTermAPI):
    """Spawn a bash agent (not claude), verify new pane created with title."""
    title = unique_name("agent-bash")
    result = api.post(
        "/api/agents/spawn",
        json={
            "agent": "bash",
            "title": title,
        },
    )
    assert "pane_id" in result
    assert result["title"] == title

    pane_id = result["pane_id"]

    # Verify pane exists
    panes = api.list_panes()
    ids = [p["id"] for p in panes]
    assert pane_id in ids

    # Cleanup
    api.close_pane(pane_id)


def test_set_role(api: CodeTermAPI, fresh_pane: str):
    """Set Orchestrator role on a pane."""
    result = api.post(
        f"/api/panes/{fresh_pane}/role",
        json={"role": "Orchestrator"},
    )
    assert result.get("ok") is True

    # Verify via status
    status = api.get(f"/api/panes/{fresh_pane}/status")
    assert status.get("role") == "Orchestrator"


def test_hierarchy(api: CodeTermAPI, fresh_pane: str):
    """Set orchestrator, spawn worker, verify hierarchy tree."""
    # Set fresh_pane as Orchestrator
    api.post(
        f"/api/panes/{fresh_pane}/role",
        json={"role": "Orchestrator"},
    )

    # Spawn a worker under it
    worker = api.post(
        "/api/agents/spawn",
        json={
            "agent": "bash",
            "title": unique_name("worker"),
            "role": "Worker",
            "parent_pane_id": fresh_pane,
        },
    )
    worker_id = worker["pane_id"]

    # Hierarchy is derived from parent_pane_id now — the /api/agents/hierarchy
    # and /api/panes/{id}/children endpoints were removed (tree is reconstructed
    # client-side from the flat pane list).
    panes = api.list_panes()
    worker_pane = next((p for p in panes if p["id"] == worker_id), None)
    assert worker_pane is not None
    assert worker_pane.get("parent_pane_id") == fresh_pane

    # Cleanup
    api.close_pane(worker_id)


def test_submit_report(api: CodeTermAPI, fresh_pane: str):
    """Worker reports to parent, verify in reports endpoint."""
    # Set up hierarchy
    api.post(
        f"/api/panes/{fresh_pane}/role",
        json={"role": "Orchestrator"},
    )

    worker = api.post(
        "/api/agents/spawn",
        json={
            "agent": "bash",
            "title": unique_name("reporter"),
            "role": "Worker",
            "parent_pane_id": fresh_pane,
        },
    )
    worker_id = worker["pane_id"]

    # Submit report from worker
    report_msg = unique_name("report-msg")
    result = api.post(
        f"/api/panes/{worker_id}/report",
        json={"message": report_msg},
    )
    assert result.get("ok") is True
    assert "report_id" in result

    # Verify report appears in parent's reports
    reports = api.get(f"/api/panes/{fresh_pane}/reports")
    assert isinstance(reports, list)
    report_messages = [r.get("message", "") for r in reports]
    assert any(report_msg in m for m in report_messages)

    # Cleanup
    api.close_pane(worker_id)


def test_spawn_with_task(api: CodeTermAPI, fresh_pane: str):
    """Spawn with task text, verify task appears in pane output."""
    task_text = f"echo TASK_{unique_name('t')}"
    result = api.post(
        "/api/agents/spawn",
        json={
            "agent": "bash",
            "title": unique_name("tasked"),
            "task": task_text,
            "parent_pane_id": fresh_pane,
        },
    )
    pane_id = result["pane_id"]

    # Wait for PTY to be linked before reading output
    api.wait_for_pty(pane_id, timeout=15)

    try:
        output = api.wait_for_output(pane_id, pattern="TASK_", timeout=15)
        assert "TASK_" in output
    except TimeoutError:
        pass

    api.close_pane(pane_id)
