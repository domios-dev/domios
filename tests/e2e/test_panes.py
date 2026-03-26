"""Pane management tests: create, split, close, rename, focus."""

import time

from helpers import CodeTermAPI, unique_name


def test_list_panes(api: CodeTermAPI):
    """At least one pane exists (from the default workspace)."""
    panes = api.list_panes()
    assert isinstance(panes, list)
    assert len(panes) >= 1


def test_split_pane_right(api: CodeTermAPI, fresh_pane: str):
    """Split horizontally (right), verify 2 panes exist."""
    panes_before = api.list_panes()
    count_before = len(panes_before)

    result = api.split_pane(fresh_pane, direction="right")
    assert "new_pane_id" in result
    new_id = result["new_pane_id"]

    panes_after = api.list_panes()
    assert len(panes_after) >= count_before + 1

    ids = [p["id"] for p in panes_after]
    assert new_id in ids

    # Cleanup the split pane
    api.close_pane(new_id)


def test_split_pane_down(api: CodeTermAPI, fresh_pane: str):
    """Split vertically (down), verify new pane created."""
    result = api.split_pane(fresh_pane, direction="down")
    assert "new_pane_id" in result
    new_id = result["new_pane_id"]

    panes = api.list_panes()
    ids = [p["id"] for p in panes]
    assert new_id in ids

    api.close_pane(new_id)


def test_close_pane(api: CodeTermAPI, fresh_pane: str):
    """Split then close the new pane."""
    result = api.split_pane(fresh_pane, direction="right")
    new_id = result["new_pane_id"]

    close_result = api.close_pane(new_id)
    assert close_result.get("ok") is True

    panes = api.list_panes()
    ids = [p["id"] for p in panes]
    assert new_id not in ids


def test_rename_pane(api: CodeTermAPI, fresh_pane: str):
    """Rename a pane and verify in the pane list."""
    new_title = unique_name("renamed")
    result = api.rename_pane(fresh_pane, new_title)
    assert result.get("ok") is True

    panes = api.list_panes()
    found = [p for p in panes if p["id"] == fresh_pane]
    assert len(found) == 1
    assert found[0]["title"] == new_title


def test_focus_pane(api: CodeTermAPI, fresh_pane: str):
    """Focus a pane (should not error)."""
    result = api.focus_pane(fresh_pane)
    assert result.get("ok") is True
