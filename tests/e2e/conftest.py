"""Shared fixtures for CodeTerm E2E tests.

Instance management:
  - If CODETERM_API_URL is set, use it directly (instance already running).
  - Otherwise, start `bash build.sh dev` from the project root and poll the
    health endpoint until it responds (up to 120 s for compilation).

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
    external_url = os.environ.get("CODETERM_API_URL")
    if external_url:
        # Normalize: strip trailing /api if present so base is just host:port
        base = external_url.rstrip("/")
        if base.endswith("/api"):
            base = base[:-4]
        _wait_for_health(f"{base}/api/health", timeout=10)
        yield base
        return

    # Start dev instance
    proc = subprocess.Popen(
        ["bash", "build.sh", "dev"],
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
    token = os.environ.get("CODETERM_API_TOKEN") or _read_token()
    client = CodeTermAPI(codeterm_instance, token=token)
    _cleanup_e2e_workspaces(client)
    yield client
    _cleanup_e2e_workspaces(client)


def _cleanup_e2e_workspaces(api: CodeTermAPI):
    """Remove any workspaces left over from previous test runs.
    Never close the last workspace — that would kill the window.
    """
    try:
        workspaces = api.get("/api/workspaces")
        e2e_ws = [ws for ws in workspaces if ws.get("name", "").startswith("e2e-ws")]
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
def fresh_pane(api: CodeTermAPI):
    """Create a disposable workspace + pane, yield pane_id, clean up after.

    The initial pane from workspace creation is an Orchestrator by default.
    """
    ws_name = unique_name("e2e-ws")
    ws = api.create_workspace(ws_name)
    ws_id = ws["id"]

    # The workspace comes with an initial tab/pane.  Grab the first pane.
    panes_before = {p["id"] for p in api.list_panes()}

    # Create a new tab to get a guaranteed fresh pane
    tab = api.create_tab(ws_id)
    pane_id = tab["pane_id"]

    # Wait for PTY to be linked (frontend creates PTY after mux-state-changed)
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        panes = api.list_panes()
        pane = next((p for p in panes if p["id"] == pane_id), None)
        if pane and pane.get("pty_id"):
            break
        time.sleep(0.5)

    # Wait for shell prompt (look for $ or > or % in output)
    try:
        api.wait_for_output(pane_id, pattern=r"[\$#%>]", timeout=15)
    except (TimeoutError, Exception):
        # Shell might not emit a standard prompt or output not yet available
        time.sleep(2)

    yield pane_id

    # Teardown: close workspace but never the last one (would kill the app)
    try:
        workspaces = api.list_workspaces()
        if len(workspaces) <= 1:
            pass  # Don't close the last workspace
        else:
            api.close_workspace(ws_id)
    except Exception:
        pass  # best effort
