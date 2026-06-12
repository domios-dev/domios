"""Cron job CRUD tests."""

from helpers import CodeTermAPI, unique_name


def _get_workspace_id(api: CodeTermAPI) -> str:
    """Get any workspace ID for cron creation."""
    workspaces = api.list_workspaces()
    assert len(workspaces) > 0, "Need at least one workspace"
    return workspaces[0]["id"]


def test_cron_create(api_only: CodeTermAPI):
    """Create a cron job, verify it appears in the list."""
    ws_id = _get_workspace_id(api_only)
    name = unique_name("cron")
    result = api_only.post(
        "/api/crons",
        json={
            "name": name,
            "workspace_id": ws_id,
            "schedule": "0 * * * *",
            "task": "echo hello",
        },
    )
    assert "id" in result
    cron_id = result["id"]

    # Verify in list
    crons = api_only.get("/api/crons")
    ids = [c["id"] for c in crons]
    assert cron_id in ids

    # Cleanup
    api_only.delete(f"/api/crons/{cron_id}")


def test_cron_update(api_only: CodeTermAPI):
    """Create then update name and schedule."""
    ws_id = _get_workspace_id(api_only)
    name = unique_name("cron-upd")
    result = api_only.post(
        "/api/crons",
        json={
            "name": name,
            "workspace_id": ws_id,
            "schedule": "0 * * * *",
            "task": "echo update-test",
        },
    )
    cron_id = result["id"]

    new_name = unique_name("cron-updated")
    updated = api_only.put(
        f"/api/crons/{cron_id}",
        json={"name": new_name, "schedule": "30 * * * *"},
    )
    assert updated["name"] == new_name

    # Verify via get
    fetched = api_only.get(f"/api/crons/{cron_id}")
    assert fetched["name"] == new_name

    api_only.delete(f"/api/crons/{cron_id}")


def test_cron_enable_disable(api_only: CodeTermAPI):
    """Toggle enabled state."""
    ws_id = _get_workspace_id(api_only)
    name = unique_name("cron-toggle")
    result = api_only.post(
        "/api/crons",
        json={
            "name": name,
            "workspace_id": ws_id,
            "schedule": "0 * * * *",
            "task": "echo toggle",
        },
    )
    cron_id = result["id"]

    # Disable
    disabled = api_only.post(f"/api/crons/{cron_id}/disable")
    assert disabled.get("enabled") is False

    # Enable
    enabled = api_only.post(f"/api/crons/{cron_id}/enable")
    assert enabled.get("enabled") is True

    api_only.delete(f"/api/crons/{cron_id}")


def test_cron_delete(api_only: CodeTermAPI):
    """Delete a cron job, verify it is gone."""
    ws_id = _get_workspace_id(api_only)
    name = unique_name("cron-del")
    result = api_only.post(
        "/api/crons",
        json={
            "name": name,
            "workspace_id": ws_id,
            "schedule": "0 * * * *",
            "task": "echo delete-me",
        },
    )
    cron_id = result["id"]

    delete_result = api_only.delete(f"/api/crons/{cron_id}")
    assert delete_result.get("ok") is True

    # Should not be in list
    crons = api_only.get("/api/crons")
    ids = [c["id"] for c in crons]
    assert cron_id not in ids


def test_cron_trigger(api_only: CodeTermAPI):
    """Trigger a cron job manually, verify history entry created."""
    ws_id = _get_workspace_id(api_only)
    name = unique_name("cron-trig")
    result = api_only.post(
        "/api/crons",
        json={
            "name": name,
            "workspace_id": ws_id,
            "schedule": "0 * * * *",
            "task": "echo triggered",
        },
    )
    cron_id = result["id"]

    # Trigger
    trigger_result = api_only.post(f"/api/crons/{cron_id}/trigger")
    assert "id" in trigger_result or "status" in trigger_result

    # Check history
    history = api_only.get(f"/api/crons/{cron_id}/history")
    assert isinstance(history, list)
    assert len(history) >= 1

    api_only.delete(f"/api/crons/{cron_id}")
