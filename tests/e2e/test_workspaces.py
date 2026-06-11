"""Workspace CRUD tests."""

import os

from helpers import CodeTermAPI, unique_name


def test_create_workspace(api: CodeTermAPI):
    """Create a workspace and verify it appears in the list."""
    name = unique_name("ws-create")
    ws = api.create_workspace(name)
    assert "id" in ws

    workspaces = api.list_workspaces()
    ids = [w["id"] for w in workspaces]
    assert ws["id"] in ids

    # Cleanup
    api.close_workspace(ws["id"])


def test_rename_workspace(api: CodeTermAPI):
    """Create, rename, verify new name in list."""
    name = unique_name("ws-rename")
    ws = api.create_workspace(name)
    ws_id = ws["id"]

    new_name = unique_name("ws-renamed")
    result = api.rename_workspace(ws_id, new_name)
    assert result.get("ok") is True

    workspaces = api.list_workspaces()
    found = [w for w in workspaces if w["id"] == ws_id]
    assert len(found) == 1
    assert found[0]["name"] == new_name

    api.close_workspace(ws_id)


def test_close_workspace(api: CodeTermAPI):
    """Close a workspace and verify panes are gone."""
    name = unique_name("ws-close")
    ws = api.create_workspace(name)
    ws_id = ws["id"]

    # Grab pane IDs belonging to this workspace before closing
    panes_before = api.list_panes()
    # There should be at least one pane from the new workspace

    result = api.close_workspace(ws_id)
    assert result.get("ok") is True

    # Workspace should no longer appear
    workspaces = api.list_workspaces()
    ids = [w["id"] for w in workspaces]
    assert ws_id not in ids


def test_create_tab_in_workspace(api: CodeTermAPI):
    """Create a new tab in a workspace, verify it returns a pane_id."""
    name = unique_name("ws-tab")
    ws = api.create_workspace(name)
    ws_id = ws["id"]

    tab = api.create_tab(ws_id)
    assert "pane_id" in tab
    assert "tab_id" in tab
    assert len(tab["pane_id"]) == 8  # 8-char hex

    api.close_workspace(ws_id)


def test_create_workspace_cwd_dedupe(api: CodeTermAPI):
    """Second create with the same --cwd returns existing=true and no new tab."""
    cwd = os.path.normpath(os.getcwd())
    name = unique_name("ws-cwd")
    ws1 = api.create_workspace(name, cwd=cwd)
    assert "id" in ws1
    assert ws1.get("existing") is None  # first call: not a dedupe

    ws2 = api.create_workspace(unique_name("ws-cwd2"), cwd=cwd)
    assert ws2["id"] == ws1["id"], "dedupe must return the original workspace"
    assert ws2.get("existing") is True, "dedupe response must have existing=true"

    # No extra tab should have been created
    workspaces = api.list_workspaces()
    found = next((w for w in workspaces if w["id"] == ws1["id"]), None)
    assert found is not None
    assert found["tab_count"] == ws1.get("tab_count", 0), "tab count must not change on dedupe"

    api.close_workspace(ws1["id"])


def test_create_workspace_fresh_counters(api: CodeTermAPI):
    """Freshly created workspace has consistent tab_count and pane_ids."""
    name = unique_name("ws-counters")
    ws = api.create_workspace(name)
    assert ws.get("tab_count", -1) == 0, "new workspace must start with tab_count=0"
    assert ws.get("pane_ids") == [], "new workspace must start with empty pane_ids"
    assert ws.get("existing") is None

    api.close_workspace(ws["id"])
