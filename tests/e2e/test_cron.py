"""Cron job CRUD tests."""

from helpers import CodeTermAPI, unique_name


def _get_workspace_id(api: CodeTermAPI) -> str:
    """Get any workspace ID for cron creation."""
    workspaces = api.list_workspaces()
    assert len(workspaces) > 0, "Need at least one workspace"
    return workspaces[0]["id"]


def test_cron_create(api: CodeTermAPI):
    """Create a cron job, verify it appears in the list."""
    ws_id = _get_workspace_id(api)
    name = unique_name("cron")
    result = api.post(
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
    crons = api.get("/api/crons")
    ids = [c["id"] for c in crons]
    assert cron_id in ids

    # Cleanup
    api.delete(f"/api/crons/{cron_id}")


def test_cron_update(api: CodeTermAPI):
    """Create then update name and schedule."""
    ws_id = _get_workspace_id(api)
    name = unique_name("cron-upd")
    result = api.post(
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
    updated = api.put(
        f"/api/crons/{cron_id}",
        json={"name": new_name, "schedule": "30 * * * *"},
    )
    assert updated["name"] == new_name

    # Verify via get
    fetched = api.get(f"/api/crons/{cron_id}")
    assert fetched["name"] == new_name

    api.delete(f"/api/crons/{cron_id}")


def test_cron_enable_disable(api: CodeTermAPI):
    """Toggle enabled state."""
    ws_id = _get_workspace_id(api)
    name = unique_name("cron-toggle")
    result = api.post(
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
    disabled = api.post(f"/api/crons/{cron_id}/disable")
    assert disabled.get("enabled") is False

    # Enable
    enabled = api.post(f"/api/crons/{cron_id}/enable")
    assert enabled.get("enabled") is True

    api.delete(f"/api/crons/{cron_id}")


def test_cron_delete(api: CodeTermAPI):
    """Delete a cron job, verify it is gone."""
    ws_id = _get_workspace_id(api)
    name = unique_name("cron-del")
    result = api.post(
        "/api/crons",
        json={
            "name": name,
            "workspace_id": ws_id,
            "schedule": "0 * * * *",
            "task": "echo delete-me",
        },
    )
    cron_id = result["id"]

    delete_result = api.delete(f"/api/crons/{cron_id}")
    assert delete_result.get("ok") is True

    # Should not be in list
    crons = api.get("/api/crons")
    ids = [c["id"] for c in crons]
    assert cron_id not in ids


def test_cron_trigger(api: CodeTermAPI):
    """Trigger a cron job manually, verify history entry created."""
    ws_id = _get_workspace_id(api)
    name = unique_name("cron-trig")
    result = api.post(
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
    trigger_result = api.post(f"/api/crons/{cron_id}/trigger")
    assert "id" in trigger_result or "status" in trigger_result

    # Check history
    history = api.get(f"/api/crons/{cron_id}/history")
    assert isinstance(history, list)
    assert len(history) >= 1

    api.delete(f"/api/crons/{cron_id}")
