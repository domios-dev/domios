"""Multi-pane prompt isolation tests."""

import time
import pytest
from helpers import CodeTermAPI, inject_screen_text, unique_name


@pytest.fixture()
def three_panes(api: CodeTermAPI):
    """Create workspace with 3 panes, yield (ws_id, [pane_ids]), cleanup."""
    ws = api.create_workspace(unique_name("e2e-multi"))
    ws_id = ws["id"]
    pane_ids = []
    for _ in range(3):
        tab = api.create_tab(ws_id, eager=True)
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

    def _set_prompt(self, api, pane_id):
        inject_screen_text(api, pane_id, (
            "Do you want to proceed?\n"
            "  > 1. Yes\n"
            "    2. No\n"
            "\nEsc to cancel \u00b7 Tab to amend\n"
        ))
        time.sleep(0.5)
        api.post(f"/api/panes/{pane_id}/hook/permission")

    def _pane(self, api, pane_id):
        return next(p for p in api.list_panes() if p["id"] == pane_id)

    def test_two_panes_independent(self, api, three_panes):
        """Two panes can have prompts simultaneously."""
        _, panes = three_panes
        self._set_prompt(api, panes[0])
        self._set_prompt(api, panes[1])
        time.sleep(0.5)
        pane_a = self._pane(api, panes[0])
        pane_b = self._pane(api, panes[1])
        assert pane_a.get("pending_prompt") is not None
        assert pane_b.get("pending_prompt") is not None

    def test_respond_one_doesnt_clear_other(self, api, three_panes):
        """Responding in pane A should not affect pane B."""
        _, panes = three_panes
        self._set_prompt(api, panes[0])
        self._set_prompt(api, panes[1])
        time.sleep(0.5)
        api.post(f"/api/panes/{panes[0]}/prompt/respond", json={"value": "1"})
        time.sleep(0.5)
        pane_list = api.list_panes()
        a = next(p for p in pane_list if p["id"] == panes[0])
        b = next(p for p in pane_list if p["id"] == panes[1])
        assert a.get("pending_prompt") is None
        assert b.get("pending_prompt") is not None

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
