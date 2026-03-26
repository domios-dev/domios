"""CodeTerm API client and test helpers."""

import re
import sys
import time

import requests


# -- ANSI stripping --------------------------------------------------------

_ANSI_RE = re.compile(
    r"""
    \x1b        # ESC
    (?:
        \[[\x30-\x3f]*[\x20-\x2f]*[\x40-\x7e]   # CSI sequence
      | \].*?(?:\x07|\x1b\\)                       # OSC sequence
      | [\x40-\x5f]                                 # Fe sequence (2-byte)
    )
    """,
    re.VERBOSE,
)


def strip_ansi(text: str) -> str:
    """Remove ANSI escape codes and carriage returns."""
    text = _ANSI_RE.sub("", text)
    text = text.replace("\r", "")
    return text


# -- Platform helpers ------------------------------------------------------

def echo_env(var_name: str) -> str:
    """Return platform-appropriate command to echo an env var."""
    if sys.platform == "win32":
        return f"echo %{var_name}%"
    return f"echo ${var_name}"


def unique_name(prefix: str = "e2e") -> str:
    """Generate a unique name for test resources."""
    import uuid
    return f"{prefix}-{uuid.uuid4().hex[:8]}"


# -- API Client ------------------------------------------------------------

class CodeTermAPI:
    """Thin wrapper around the CodeTerm REST API."""

    def __init__(self, base_url: str, token: str | None = None):
        self.base = base_url.rstrip("/")
        self.s = requests.Session()
        if token:
            self.s.headers["Authorization"] = f"Bearer {token}"

    # -- Generic HTTP -------------------------------------------------------

    def get(self, path: str, **kw) -> dict:
        r = self.s.get(f"{self.base}{path}", **kw)
        r.raise_for_status()
        return r.json()

    def post(self, path: str, **kw) -> dict:
        r = self.s.post(f"{self.base}{path}", **kw)
        r.raise_for_status()
        return r.json()

    def put(self, path: str, **kw) -> dict:
        r = self.s.put(f"{self.base}{path}", **kw)
        r.raise_for_status()
        return r.json()

    def delete(self, path: str, **kw) -> dict:
        r = self.s.delete(f"{self.base}{path}", **kw)
        r.raise_for_status()
        return r.json()

    # -- High-level helpers -------------------------------------------------

    def exec_cmd(
        self,
        pane_id: str,
        command: str,
        timeout_ms: int = 10000,
        strip_ansi_flag: bool = True,
    ) -> dict:
        """POST /api/panes/{id}/exec — send command, wait for output.

        Returns dict with keys: output, offset, and optionally timeout.
        """
        return self.post(
            f"/api/panes/{pane_id}/exec",
            json={
                "command": command,
                "timeout_ms": timeout_ms,
                "strip_ansi": strip_ansi_flag,
            },
        )

    def send_text(
        self,
        pane_id: str,
        text: str | None = None,
        enter: bool = True,
        batch: list[dict] | None = None,
    ) -> dict:
        """POST /api/panes/{id}/send — send text / batch to PTY."""
        body: dict = {}
        if batch is not None:
            body["batch"] = batch
        else:
            if text is not None:
                body["text"] = text
            body["enter"] = enter
        return self.post(f"/api/panes/{pane_id}/send", json=body)

    def get_output(
        self,
        pane_id: str,
        offset: int = 0,
        strip_ansi_flag: bool = False,
    ) -> dict:
        """GET /api/panes/{id}/output — raw or stripped output buffer."""
        params: dict = {}
        if offset:
            params["offset"] = offset
        if strip_ansi_flag:
            params["strip_ansi"] = "true"
        return self.get(f"/api/panes/{pane_id}/output", params=params)

    def wait_for_output(
        self,
        pane_id: str,
        pattern: str | None = None,
        timeout: float = 10,
        strip_ansi_flag: bool = True,
    ) -> str:
        """Poll GET /api/panes/{id}/output until *pattern* appears or timeout.

        Returns the full stripped output text.  Raises TimeoutError on failure.
        """
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            data = self.get_output(pane_id, strip_ansi_flag=strip_ansi_flag)
            text = data.get("output", "")
            if pattern is None:
                return text
            if re.search(pattern, text):
                return text
            time.sleep(0.3)
        raise TimeoutError(
            f"Pattern {pattern!r} not found in pane {pane_id} output within {timeout}s"
        )

    def get_screen(self, pane_id: str) -> str:
        """GET /api/panes/{id}/screen — visible terminal text."""
        data = self.get(f"/api/panes/{pane_id}/screen")
        return data.get("screen", "")

    # -- Workspace / Pane CRUD shortcuts ------------------------------------

    def create_workspace(self, name: str) -> dict:
        return self.post("/api/workspaces", json={"name": name})

    def list_workspaces(self) -> list[dict]:
        return self.get("/api/workspaces")

    def rename_workspace(self, ws_id: str, name: str) -> dict:
        return self.post(f"/api/workspaces/{ws_id}/rename", json={"name": name})

    def close_workspace(self, ws_id: str) -> dict:
        return self.post(f"/api/workspaces/{ws_id}/close")

    def create_tab(self, ws_id: str) -> dict:
        return self.post(f"/api/workspaces/{ws_id}/tabs")

    def list_panes(self) -> list[dict]:
        return self.get("/api/panes")

    def split_pane(self, pane_id: str, direction: str = "right") -> dict:
        return self.post(
            f"/api/panes/{pane_id}/split", json={"direction": direction}
        )

    def close_pane(self, pane_id: str) -> dict:
        return self.post(f"/api/panes/{pane_id}/close")

    def rename_pane(self, pane_id: str, title: str) -> dict:
        return self.post(f"/api/panes/{pane_id}/rename", json={"title": title})

    def focus_pane(self, pane_id: str) -> dict:
        return self.post(f"/api/panes/{pane_id}/focus")

    def wait_for_pty(self, pane_id: str, timeout: float = 20) -> bool:
        """Wait until a pane has a pty_id (PTY linked by frontend)."""
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            panes = self.list_panes()
            pane = next((p for p in panes if p["id"] == pane_id), None)
            if pane and pane.get("pty_id"):
                return True
            time.sleep(0.5)
        return False

    def wait_for_screen(
        self,
        pane_id: str,
        pattern: str,
        timeout: float = 30,
    ) -> str:
        """Poll GET /api/panes/{id}/screen until pattern appears. Returns screen text."""
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                text = self.get_screen(pane_id)
                if re.search(pattern, text):
                    return text
            except Exception:
                pass
            time.sleep(1)
        raise TimeoutError(
            f"Pattern {pattern!r} not found on screen of pane {pane_id} within {timeout}s"
        )
