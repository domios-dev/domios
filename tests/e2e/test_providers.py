"""Provider spawn, detection, and model-selection coverage.

These tests boot real provider CLIs but do not send prompts. They assert that
CodeTerm's spawn metadata, provider detection, and TUI boot surfaces are wired
for every first-party agent provider.
"""

import shutil
import time

import pytest
import requests
from helpers import CodeTermAPI, unique_name

pytestmark = pytest.mark.llm


PROVIDERS = [
    pytest.param("claude", "haiku", ["haiku"], id="claude"),
    pytest.param("codex", "gpt-5.4-mini", ["gpt-5.4-mini", "gpt 5.4 mini", "mini"], id="codex"),
    pytest.param("opencode", "opencode/big-pickle", ["opencode/big-pickle", "big-pickle"], id="opencode"),
]

AUTH_FAILURE_MARKERS = (
    "not authenticated",
    "not logged in",
    "login required",
    "please log in",
    "please login",
    "sign in",
    "api key",
    "authentication failed",
)


def _provider_binary(provider: str) -> str | None:
    candidates = [provider, f"{provider}.exe", f"{provider}.cmd", f"{provider}.ps1"]
    return next((path for name in candidates if (path := shutil.which(name))), None)


def _tab(api: CodeTermAPI, tab_id: str) -> dict | None:
    return next((p for p in api.list_tabs() if p.get("id") == tab_id), None)


def _wait_for_tab(api: CodeTermAPI, tab_id: str, timeout: float = 20) -> dict:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        tab = _tab(api, tab_id)
        if tab is not None:
            return tab
        time.sleep(0.5)
    raise AssertionError(f"tab {tab_id} was not created")


def _wait_for_nontrivial_output(api: CodeTermAPI, tab_id: str, timeout: float = 90) -> str:
    deadline = time.monotonic() + timeout
    last_text = ""
    while time.monotonic() < deadline:
        try:
            data = api.get_output(tab_id, strip_ansi_flag=True)
            text = data.get("output", "")
            last_text = text or last_text
            compact = "".join(text.split())
            if len(compact) >= 20:
                return text
        except Exception:
            pass
        try:
            text = api.get_screen(tab_id)
            last_text = text or last_text
            compact = "".join(text.split())
            if len(compact) >= 20:
                return text
        except Exception:
            pass
        time.sleep(1)
    raise AssertionError(
        f"tab {tab_id} did not render non-trivial TUI output; last output: {last_text[-300:]}"
    )


def _skip_if_auth_blocked(provider: str, text: str) -> None:
    lower = text.lower()
    if any(marker in lower for marker in AUTH_FAILURE_MARKERS):
        pytest.skip(f"{provider} CLI is installed but not authenticated")


def _wait_for_provider_detection(
    api: CodeTermAPI,
    tab_id: str,
    provider: str,
    model_markers: list[str],
    timeout: float = 120,
) -> dict:
    deadline = time.monotonic() + timeout
    last_tab = None
    while time.monotonic() < deadline:
        last_tab = _tab(api, tab_id)
        if last_tab is not None:
            detected = last_tab.get("agent_type")
            session_model = last_tab.get("agent_session_model")
            if detected == provider and _model_matches(session_model, model_markers):
                return last_tab
        time.sleep(1)
    raise AssertionError(
        "provider/model detection did not settle for "
        f"{provider}; last tab state: {last_tab}"
    )


def _model_matches(session_model: str | None, markers: list[str]) -> bool:
    if not session_model:
        return False
    lower = session_model.lower()
    return any(marker.lower() in lower for marker in markers)


def _spawn_provider(
    api: CodeTermAPI,
    provider: str,
    model: str,
    workspace_id: str,
) -> dict:
    if _provider_binary(provider) is None:
        pytest.skip(f"{provider} CLI is not installed")
    try:
        return api.post(
            "/api/agents/spawn",
            json={
                "agent": provider,
                "model": model,
                "title": unique_name(f"provider-{provider}"),
                "workspace_id": workspace_id,
                "skip_permissions": True,
                "activate": True,
            },
        )
    except requests.HTTPError as exc:
        response = exc.response
        detail = response.text if response is not None else str(exc)
        if any(marker in detail.lower() for marker in AUTH_FAILURE_MARKERS):
            pytest.skip(f"{provider} CLI is not authenticated: {detail}")
        pytest.fail(f"{provider} spawn failed: {detail}")


@pytest.mark.parametrize(("provider", "model", "model_markers"), PROVIDERS)
def test_provider_spawn_detection_model_and_tui(
    api: CodeTermAPI,
    provider: str,
    model: str,
    model_markers: list[str],
):
    ws_id = None
    tab_id = None
    try:
        ws = api.create_workspace(unique_name("e2e-ws-provider"))
        ws_id = ws["id"]
        result = _spawn_provider(api, provider, model, ws_id)
        tab_id = result.get("tab_id")

        assert result.get("agent") == provider
        assert tab_id, f"{provider} spawn response did not include tab_id: {result}"
        _wait_for_tab(api, tab_id)
        output = _wait_for_nontrivial_output(api, tab_id)
        _skip_if_auth_blocked(provider, output)
        tab = _wait_for_provider_detection(api, tab_id, provider, model_markers)

        assert tab.get("agent_type") == provider
        assert _model_matches(tab.get("agent_session_model"), model_markers), tab
    finally:
        if tab_id:
            try:
                api.close_tab(tab_id)
            except Exception:
                pass
        if ws_id:
            try:
                if len(api.list_workspaces()) > 1:
                    api.close_workspace(ws_id)
            except Exception:
                pass


def test_plain_shell_tab_has_no_agent_detection(api_only: CodeTermAPI, api_tab: str):
    tab = _wait_for_tab(api_only, api_tab)
    assert tab.get("agent_type") in (None, "")
    assert tab.get("agent_session_model") in (None, "")
