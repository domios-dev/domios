"""Agent hierarchy tests — roles and reports without LLM providers."""

from helpers import CodeTermAPI, unique_name


def create_shell_pane(api: CodeTermAPI, title: str) -> str:
    result = api.post("/api/panes", json={"title": title, "eager": True})
    pane_id = result["pane_id"]
    assert pane_id
    assert result.get("pty_id")
    return pane_id


def test_create_shell_pane(api: CodeTermAPI):
    """Create a shell pane and verify it appears with the requested title."""
    title = unique_name("agent-shell")
    pane_id = create_shell_pane(api, title)
    try:
        pane = next((p for p in api.list_panes() if p["id"] == pane_id), None)
        assert pane is not None
        assert pane.get("title") == title
    finally:
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

    worker_id = create_shell_pane(api, unique_name("worker"))
    try:
        api.post(
            f"/api/panes/{worker_id}/role",
            json={"role": "Worker", "parent_pane_id": fresh_pane},
        )

        # Hierarchy is derived from parent_pane_id now — the /api/agents/hierarchy
        # and /api/panes/{id}/children endpoints were removed (tree is reconstructed
        # client-side from the flat pane list).
        panes = api.list_panes()
        worker_pane = next((p for p in panes if p["id"] == worker_id), None)
        assert worker_pane is not None
        assert worker_pane.get("parent_pane_id") == fresh_pane
    finally:
        api.close_pane(worker_id)


def test_submit_report(api: CodeTermAPI, fresh_pane: str):
    """Worker reports to parent, verify in reports endpoint."""
    # Set up hierarchy
    api.post(
        f"/api/panes/{fresh_pane}/role",
        json={"role": "Orchestrator"},
    )

    worker_id = create_shell_pane(api, unique_name("reporter"))
    try:
        api.post(
            f"/api/panes/{worker_id}/role",
            json={"role": "Worker", "parent_pane_id": fresh_pane},
        )

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
    finally:
        api.close_pane(worker_id)


def test_shell_pane_accepts_sent_task_text(api: CodeTermAPI):
    """Send task text to a shell pane and verify the command executes."""
    task_marker = f"TASK_{unique_name('t')}"
    pane_id = create_shell_pane(api, unique_name("tasked"))
    try:
        api.send_text(pane_id, text=f"echo {task_marker}")
        output = api.wait_for_output(pane_id, pattern=task_marker, timeout=15)
        assert task_marker in output
    finally:
        api.close_pane(pane_id)
