"""Agent hierarchy tests — roles and reports without LLM providers."""

from helpers import CodeTermAPI, shell_cmd, unique_name


def create_api_pane(api: CodeTermAPI, title: str) -> str:
    result = api.post("/api/panes", json={"title": title, "eager": False})
    pane_id = result["pane_id"]
    assert pane_id
    return pane_id


def test_create_api_pane(api_only: CodeTermAPI):
    """Create a lazy pane and verify it appears with the requested title."""
    title = unique_name("agent-shell")
    pane_id = create_api_pane(api_only, title)
    try:
        pane = next((p for p in api_only.list_panes() if p["id"] == pane_id), None)
        assert pane is not None
        assert pane.get("title") == title
    finally:
        api_only.close_pane(pane_id)


def test_set_role(api_only: CodeTermAPI, api_pane: str):
    """Set Orchestrator role on a pane."""
    result = api_only.post(
        f"/api/panes/{api_pane}/role",
        json={"role": "Orchestrator"},
    )
    assert result.get("ok") is True

    # Verify via status
    status = api_only.get(f"/api/panes/{api_pane}/status")
    assert status.get("role") == "Orchestrator"


def test_hierarchy(api_only: CodeTermAPI, api_pane: str):
    """Set orchestrator, spawn worker, verify hierarchy tree."""
    api_only.post(
        f"/api/panes/{api_pane}/role",
        json={"role": "Orchestrator"},
    )

    worker_id = create_api_pane(api_only, unique_name("worker"))
    try:
        api_only.post(
            f"/api/panes/{worker_id}/role",
            json={"role": "Worker", "parent_pane_id": api_pane},
        )

        # Hierarchy is derived from parent_pane_id now — the /api/agents/hierarchy
        # and /api/panes/{id}/children endpoints were removed (tree is reconstructed
        # client-side from the flat pane list).
        panes = api_only.list_panes()
        worker_pane = next((p for p in panes if p["id"] == worker_id), None)
        assert worker_pane is not None
        assert worker_pane.get("parent_pane_id") == api_pane
    finally:
        api_only.close_pane(worker_id)


def test_submit_report(api_only: CodeTermAPI, api_pane: str):
    """Worker reports to parent, verify in reports endpoint."""
    api_only.post(
        f"/api/panes/{api_pane}/role",
        json={"role": "Orchestrator"},
    )

    worker_id = create_api_pane(api_only, unique_name("reporter"))
    try:
        api_only.post(
            f"/api/panes/{worker_id}/role",
            json={"role": "Worker", "parent_pane_id": api_pane},
        )

        # Submit report from worker
        report_msg = unique_name("report-msg")
        result = api_only.post(
            f"/api/panes/{worker_id}/report",
            json={"message": report_msg},
        )
        assert result.get("ok") is True
        assert "report_id" in result

        # Verify report appears in parent's reports
        reports = api_only.get(f"/api/panes/{api_pane}/reports")
        assert isinstance(reports, list)
        report_messages = [r.get("message", "") for r in reports]
        assert any(report_msg in m for m in report_messages)
    finally:
        api_only.close_pane(worker_id)


def test_shell_pane_accepts_sent_task_text(api: CodeTermAPI, shared_eager_pane: str):
    """Send task text to a shell pane and verify the command executes."""
    task_marker = f"TASK_{unique_name('t')}"
    split = len(task_marker) // 2
    left, right = task_marker[:split], task_marker[split:]
    api.send_text(
        shared_eager_pane,
        text=shell_cmd(f"echo '{left}''{right}'", f"Write-Output ('{left}' + '{right}')"),
    )
    output = api.wait_for_output(shared_eager_pane, pattern=task_marker, timeout=15)
    assert task_marker in output
