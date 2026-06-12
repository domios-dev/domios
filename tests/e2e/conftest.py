"""Shared fixtures for CodeTerm E2E tests.

Instance management:
  - Always target the dev daemon on 127.0.0.1:7686.
  - If it is not running, start it through Git Bash and poll the health
    endpoint until it responds (up to 120 s for compilation).

Token:
  - Read from ~/.codeterm-dev/config.toml after the instance is ready.
"""

import os
import re
import signal
import subprocess
import sys
import time
from pathlib import Path

import pytest
import requests

from helpers import CodeTermAPI, unique_name

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

PROJECT_ROOT = Path(__file__).resolve().parents[2]
DEV_PORT = 7686
DEV_URL = f"http://127.0.0.1:{DEV_PORT}"
HEALTH_ENDPOINT = f"{DEV_URL}/api/health"
DEV_CONFIG_PATH = Path.home() / ".codeterm-dev" / "config.toml"
GIT_BASH = Path("C:/Program Files/Git/bin/bash.exe")
STARTUP_TIMEOUT = 120  # seconds — includes cargo build time


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _read_token() -> str | None:
    """Parse token from ~/.codeterm-dev/config.toml [api] section."""
    if not DEV_CONFIG_PATH.exists():
        return None
    text = DEV_CONFIG_PATH.read_text()
    # Simple TOML parser — look for token = "..." under [api]
    in_api = False
    for line in text.splitlines():
        stripped = line.strip()
        if stripped == "[api]":
            in_api = True
            continue
        if stripped.startswith("[") and in_api:
            break  # next section
        if in_api:
            m = re.match(r'token\s*=\s*"([^"]*)"', stripped)
            if m:
                return m.group(1)
    return None


def _resolve_token() -> str | None:
    dev_token = _read_token()
    os.environ["CODETERM_API_TOKEN"] = dev_token or ""
    return dev_token


def _force_dev_env() -> None:
    os.environ["CODETERM_API_URL"] = DEV_URL
    os.environ["CODETERM_API_PORT"] = str(DEV_PORT)


def _wait_for_health(url: str, timeout: float) -> None:
    """Poll health endpoint until it responds with 200."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            r = requests.get(url, timeout=2)
            if r.status_code == 200:
                return
        except requests.ConnectionError:
            pass
        time.sleep(1)
    raise TimeoutError(f"CodeTerm did not become healthy within {timeout}s")


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

@pytest.fixture(scope="session")
def codeterm_instance():
    """Provide a running CodeTerm dev instance URL.

    Yields the base URL (e.g. http://127.0.0.1:7686).
    """
    _force_dev_env()
    try:
        _wait_for_health(HEALTH_ENDPOINT, timeout=10)
        yield DEV_URL
        return
    except TimeoutError:
        pass

    # Start dev instance
    proc = subprocess.Popen(
        [str(GIT_BASH), "build.sh", "dev"],
        cwd=str(PROJECT_ROOT),
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        preexec_fn=os.setsid if sys.platform != "win32" else None,
    )

    try:
        _wait_for_health(HEALTH_ENDPOINT, timeout=STARTUP_TIMEOUT)
        yield DEV_URL
    finally:
        # Kill the entire process group
        if sys.platform == "win32":
            proc.terminate()
        else:
            os.killpg(os.getpgid(proc.pid), signal.SIGTERM)
        proc.wait(timeout=10)


@pytest.fixture(scope="session")
def api(codeterm_instance):
    """Session-scoped CodeTermAPI client. Cleans up leftover e2e workspaces on start/end."""
    _force_dev_env()
    token = _resolve_token()
    client = CodeTermAPI(codeterm_instance, token=token)
    _cleanup_e2e_workspaces(client)
    yield client
    _cleanup_e2e_workspaces(client)


@pytest.fixture(scope="session")
def api_only(api: CodeTermAPI):
    """REST-only client for tests that do not need a terminal pane."""
    return api


@pytest.fixture(scope="session", autouse=True)
def sweep_e2e_workspaces(api):
    """Pre-run sweep for workspaces leaked by interrupted or failed tests."""
    _cleanup_e2e_workspaces(api)


def _cleanup_e2e_workspaces(api: CodeTermAPI):
    """Remove any workspaces left over from previous test runs.
    Never close the last workspace — that would kill the window.
    """
    try:
        workspaces = api.get("/api/workspaces")
        prefixes = ("e2e-ws-", "e2e-ws-eager-")
        e2e_ws = [
            ws
            for ws in workspaces
            if ws.get("name", "").startswith(prefixes)
        ]
        non_e2e_count = len(workspaces) - len(e2e_ws)
        for ws in e2e_ws:
            # Don't close if it would leave zero workspaces
            if non_e2e_count + len(e2e_ws) <= 1:
                break
            try:
                api.close_workspace(ws["id"])
                e2e_ws = e2e_ws[1:]  # track remaining
            except Exception:
                pass
    except Exception:
        pass


@pytest.fixture()
def api_pane(api_only: CodeTermAPI):
    """Create a lazy pane for API metadata tests; no PTY is requested."""
    ws_id = None
    try:
        ws = api_only.create_workspace(unique_name("e2e-ws-api"))
        ws_id = ws["id"]
        tab = api_only.create_tab(ws_id, eager=False)
        yield tab["pane_id"]
    finally:
        if ws_id is not None:
            try:
                workspaces = api_only.list_workspaces()
                if len(workspaces) > 1:
                    api_only.close_workspace(ws_id)
            except Exception:
                pass


@pytest.fixture(scope="module")
def shared_eager_pane(api: CodeTermAPI):
    """Module-scoped PTY pane for read-only shell probes with unique markers."""
    ws_id = None
    try:
        ws = api.create_workspace(unique_name("e2e-ws-shared"))
        ws_id = ws["id"]
        tab = api.create_tab(ws_id, eager=True)
        pane_id = tab["pane_id"]
        api.wait_for_pty(pane_id, timeout=15)
        try:
            api.wait_for_output(pane_id, pattern=r"[\$#%>]", timeout=8)
        except (TimeoutError, Exception):
            time.sleep(2)
        yield pane_id
    finally:
        if ws_id is not None:
            try:
                workspaces = api.list_workspaces()
                if len(workspaces) > 1:
                    api.close_workspace(ws_id)
            except Exception:
                pass


@pytest.fixture()
def fresh_pane(api: CodeTermAPI):
    """Create a disposable workspace + pane, yield pane_id, clean up after.

    The initial pane from workspace creation is an Orchestrator by default.
    """
    ws_id = None
    try:
        ws_name = unique_name("e2e-ws")
        ws = api.create_workspace(ws_name)
        ws_id = ws["id"]

        # Create a new tab to get a guaranteed fresh pane
        tab = api.create_tab(ws_id, eager=True)
        pane_id = tab["pane_id"]

        api.wait_for_pty(pane_id, timeout=15)

        # Wait for shell prompt (look for $ or > or % in output)
        try:
            api.wait_for_output(pane_id, pattern=r"[\$#%>]", timeout=8)
        except (TimeoutError, Exception):
            # Shell might not emit a standard prompt or output not yet available
            time.sleep(2)

        yield pane_id
    finally:
        if ws_id is not None:
            try:
                workspaces = api.list_workspaces()
                if len(workspaces) > 1:
                    api.close_workspace(ws_id)
            except Exception:
                pass  # best effort
