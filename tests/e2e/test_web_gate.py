"""Fast web gate auth-boundary coverage."""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Any

import pytest
import requests


SPA_MARKERS = ('id="root"', 'type="module"', "./assets/index-")
REJECTION_BODY = "Invalid or missing token"


@dataclass(frozen=True)
class RouteCase:
    method: str
    path: str
    json: dict[str, Any] | None = None
    files: dict[str, tuple[str, bytes, str]] | None = None


PROTECTED_WEB_ROUTES = [
    RouteCase("GET", "/api/mux/state"),
    RouteCase("POST", "/api/workspaces/ws_missing/tabs/tab_missing/focus"),
    RouteCase("POST", "/api/workspaces/ws_missing/tabs/tab_missing/close"),
    RouteCase("POST", "/api/panes/pane_missing/chat-view", json={"enabled": True}),
    RouteCase("GET", "/api/mesh/peers/peer_missing/services"),
    RouteCase("GET", "/api/events/stream"),
    RouteCase("GET", "/api/mesh/web-token"),
    RouteCase("POST", "/api/transcribe-voice", json={"path": "C:/tmp/voice.webm"}),
    RouteCase("POST", "/api/panes/pane_missing/chat-raw/start"),
    RouteCase("POST", "/api/panes/pane_missing/chat-raw/stop"),
    RouteCase(
        "POST",
        "/api/panes/pane_missing/chat-raw/backfill",
        json={"before_offset": 0, "max_bytes": 1024},
    ),
    RouteCase(
        "POST",
        "/api/upload",
        files={"file": ("web-gate.txt", b"web gate", "text/plain")},
    ),
    RouteCase("GET", "/api/pty/pty_missing/read"),
    RouteCase("POST", "/api/pty/pty_missing/write", json={"data": "x"}),
    RouteCase("POST", "/api/pty/pty_missing/resize", json={"cols": 80, "rows": 24}),
    RouteCase(
        "POST",
        "/api/panes/pane_missing/resize-owner",
        json={"pty_id": "pty_missing", "client_id": "client", "cols": 80, "rows": 24},
    ),
    RouteCase("POST", "/web/manifest-bootstrap"),
]


ALLOWED_ANONYMOUS_WEB_ROUTES = [
    "/web/",
    "/web/deep/link",
    "/web/manifest.webmanifest",
    "/web/icons/pwa-192.png",
]


def _token_from_api_fixture(api) -> str:
    auth = api.s.headers.get("Authorization", "")
    assert auth.startswith("Bearer "), "api fixture must use the dev config bearer token"
    token = auth.removeprefix("Bearer ")
    assert token
    return token


def _request(base: str, route: RouteCase, token: str | None = None) -> requests.Response:
    headers = {}
    if token is not None:
        headers["Authorization"] = f"Bearer {token}"
    return requests.request(
        route.method,
        f"{base}{route.path}",
        headers=headers,
        json=route.json,
        files=route.files,
        timeout=10,
    )


def _assert_not_spa(response: requests.Response, expected_token: str) -> None:
    assert response.status_code == 401
    assert response.text == REJECTION_BODY
    assert expected_token not in response.text
    for marker in SPA_MARKERS:
        assert marker not in response.text


@pytest.mark.parametrize("route", PROTECTED_WEB_ROUTES, ids=lambda r: f"{r.method} {r.path}")
def test_web_gate_rejects_missing_token_without_serving_spa(api_only, route: RouteCase):
    expected_token = _token_from_api_fixture(api_only)

    response = _request(api_only.base, route)

    _assert_not_spa(response, expected_token)


@pytest.mark.parametrize("route", PROTECTED_WEB_ROUTES, ids=lambda r: f"{r.method} {r.path}")
def test_web_gate_rejects_bad_token_without_serving_spa(api_only, route: RouteCase):
    expected_token = _token_from_api_fixture(api_only)

    response = _request(api_only.base, route, token="garbage-expired-token")

    _assert_not_spa(response, expected_token)


def test_web_gate_valid_codeterm_web_link_token_serves_spa_and_api(api_only):
    token = _token_from_api_fixture(api_only)

    spa = requests.get(f"{api_only.base}/web/#token={token}", timeout=10)
    assert spa.status_code == 200
    assert len(spa.text) > 1000
    for marker in SPA_MARKERS:
        assert marker in spa.text

    manifest = requests.get(
        f"{api_only.base}/web/manifest.webmanifest",
        params={"token": token},
        timeout=10,
    )
    assert manifest.status_code == 200
    assert manifest.headers["content-type"].startswith("application/manifest+json")
    assert manifest.json()["start_url"] == f"/web/#token={token}"

    state = requests.get(
        f"{api_only.base}/api/mux/state",
        headers={"Authorization": f"Bearer {token}"},
        timeout=10,
    )
    assert state.status_code == 200, state.text
    payload = state.json()
    assert set(payload) == {
        "workspaces",
        "panes",
        "active_workspace_id",
        "focused_pane_id",
    }
    assert isinstance(payload["workspaces"], list)
    assert isinstance(payload["panes"], dict)


@pytest.mark.parametrize("path", ALLOWED_ANONYMOUS_WEB_ROUTES)
def test_allowed_anonymous_web_surface_is_explicit(api_only, path: str):
    response = requests.get(f"{api_only.base}{path}", timeout=10)

    assert response.status_code == 200
    if path.endswith(".webmanifest"):
        body = response.json()
        assert body["start_url"] == "/web/"
        assert body["scope"] == "/web/"
    elif path.endswith(".png"):
        assert response.headers["content-type"].startswith("image/png")
        assert len(response.content) > 1000
    else:
        assert len(response.text) > 1000
        for marker in SPA_MARKERS:
            assert marker in response.text


def test_allowed_anonymous_static_asset_is_explicit(api_only):
    index = requests.get(f"{api_only.base}/web/", timeout=10)
    assert index.status_code == 200
    match = re.search(r'src="\./(assets/index-[^"]+\.js)"', index.text)
    assert match, index.text[:500]

    asset = requests.get(f"{api_only.base}/web/{match.group(1)}", timeout=10)

    assert asset.status_code == 200
    assert asset.headers["content-type"].startswith("text/javascript")
    assert len(asset.text) > 100_000


def test_manifest_bad_token_falls_back_without_leaking_expected_token(api_only):
    expected_token = _token_from_api_fixture(api_only)

    response = requests.get(
        f"{api_only.base}/web/manifest.webmanifest",
        params={"token": "garbage-expired-token"},
        timeout=10,
    )

    assert response.status_code == 200
    assert response.json()["start_url"] == "/web/"
    assert expected_token not in response.text
