"""Transcript read endpoint: GET /api/tabs/{id}/chat.

The CLI (`codeterm tab chat`) and the mesh RPC map are the only non-UI readers
of chat history. They decode a non-JSON body to `{}`, so a missing route is
indistinguishable from "no transcript" at the call site — these tests assert the
route exists and answers honestly.
"""

import uuid

import requests

from helpers import CodeTermAPI


def _chat(api: CodeTermAPI, tab_id: str, since: int = 0) -> requests.Response:
    return api.s.get(f"{api.base}/api/tabs/{tab_id}/chat", params={"since": since})


def test_chat_read_is_mounted_and_returns_json(api_only: CodeTermAPI, api_tab: str):
    """A tab with no bound agent session reads as an honest empty transcript."""
    resp = _chat(api_only, api_tab)
    assert resp.status_code == 200, resp.text
    body = resp.json()
    assert body["messages"] == []
    assert body["count"] == 0
    assert body["pending"] is True


def test_chat_read_of_unknown_tab_never_widens_to_a_neighbour(api_only: CodeTermAPI):
    """An unknown tab id is 404 — never another tab's transcript."""
    resp = _chat(api_only, f"missing-{uuid.uuid4().hex}")
    assert resp.status_code == 404, resp.text


def test_chat_read_paginates_from_since(api_only: CodeTermAPI, api_tab: str):
    """`since` beyond the transcript end is empty, not an error."""
    resp = _chat(api_only, api_tab, since=10_000)
    assert resp.status_code == 200, resp.text
    assert resp.json()["messages"] == []
