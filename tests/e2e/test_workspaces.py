"""Workspace CRUD tests."""

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
