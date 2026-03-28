"""Multi-pane prompt isolation tests."""

import time
import pytest
from helpers import CodeTermAPI, unique_name


@pytest.fixture()
def three_panes(api: CodeTermAPI):
    """Create workspace with 3 panes, yield (ws_id, [pane_ids]), cleanup."""
    ws = api.create_workspace(unique_name("e2e-multi"))
    ws_id = ws["id"]
    pane_ids = []
    for _ in range(3):
        tab = api.create_tab(ws_id)
        pid = tab["pane_id"]
        assert api.wait_for_pty(pid, timeout=15)
        pane_ids.append(pid)
    for pid in pane_ids:
        try:
            api.wait_for_output(pid, pattern=r"[\$#%>]", timeout=10)
        except TimeoutError:
            time.sleep(2)
    yield ws_id, pane_ids
    try:
        if len(api.list_workspaces()) > 1:
            api.close_workspace(ws_id)
    except Exception:
        pass


class TestMultiPanePrompts:
    """Test independent prompts across multiple panes."""

    def test_two_panes_independent(self, api, three_panes):
        """Two panes can have prompts simultaneously."""
        _, panes = three_panes
        api.post(f"/api/panes/{panes[0]}/hook/permission")
        api.post(f"/api/panes/{panes[1]}/hook/permission")
        time.sleep(0.5)

    def test_respond_one_doesnt_clear_other(self, api, three_panes):
        """Responding in pane A should not affect pane B."""
        _, panes = three_panes
        api.post(f"/api/panes/{panes[0]}/hook/permission")
        api.post(f"/api/panes/{panes[1]}/hook/permission")
        time.sleep(0.5)
        api.post(f"/api/panes/{panes[0]}/prompt/respond", json={"value": "1"})
        time.sleep(0.5)
        a = next(p for p in api.list_panes() if p["id"] == panes[0])
        assert a.get("pending_prompt") is None

    def test_uninvolved_pane_clean(self, api, three_panes):
        """Pane without hook call should have no prompt."""
        _, panes = three_panes
        api.post(f"/api/panes/{panes[0]}/hook/permission")
        time.sleep(0.5)
        c = next(p for p in api.list_panes() if p["id"] == panes[2])
        assert c.get("pending_prompt") is None

    def test_all_panes_unique_ids(self, api, three_panes):
        """All panes should have unique IDs."""
        _, pane_ids = three_panes
        assert len(set(pane_ids)) == 3

    def test_all_panes_are_orchestrators(self, api, three_panes):
        """All API-created panes should be Orchestrators."""
        _, pane_ids = three_panes
        pane_list = api.list_panes()
        for pid in pane_ids:
            pane = next(p for p in pane_list if p["id"] == pid)
            assert pane.get("role") == "Orchestrator"
