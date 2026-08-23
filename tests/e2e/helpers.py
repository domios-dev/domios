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

IS_WINDOWS = sys.platform == "win32"


def echo_env(var_name: str) -> str:
    """Return platform-appropriate command to echo an env var."""
    if IS_WINDOWS:
        return f"Write-Output $env:{var_name}"
    return f"echo ${var_name}"


def shell_cmd(unix: str, win: str | None = None) -> str:
    """Return platform-appropriate shell command.

    If *win* is None, returns *unix* on all platforms (for commands that
    work in both bash and PowerShell, like ``echo``).
    """
    if IS_WINDOWS and win is not None:
        return win
    return unix


def tmp_path(filename: str) -> str:
    """Cross-platform temp file path."""
    import tempfile
    from pathlib import Path
    return str(Path(tempfile.gettempdir()) / filename)


def inject_screen_text(api: "CodeTermAPI", tab_id: str, text: str) -> None:
    """Output exact text to the terminal screen via a temp file.

    Writes *text* to a temp file then runs ``cat`` (Unix) or
    ``Get-Content -Raw`` (Windows) so the output appears cleanly
    without shell continuation markers like PowerShell's ``>>``.
    """
    import os
    import tempfile

    fd, path = tempfile.mkstemp(suffix=".txt", text=True)
    with os.fdopen(fd, "w", encoding="utf-8") as f:
        f.write(text)
    if IS_WINDOWS:
        api.send_text(tab_id, text=f'Get-Content -Raw "{path}"')
    else:
        api.send_text(tab_id, text=f'cat "{path}"')


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

    @staticmethod
    def _raise_for_status(r: requests.Response) -> None:
        try:
            r.raise_for_status()
        except requests.HTTPError as e:
            raise requests.HTTPError(f"{e} | body: {r.text[:500]}", response=r) from e

    def get(self, path: str, **kw) -> dict:
        r = self.s.get(f"{self.base}{path}", **kw)
        self._raise_for_status(r)
        return r.json()

    def post(self, path: str, **kw) -> dict:
        r = self.s.post(f"{self.base}{path}", **kw)
        self._raise_for_status(r)
        return r.json()

    def put(self, path: str, **kw) -> dict:
        r = self.s.put(f"{self.base}{path}", **kw)
        self._raise_for_status(r)
        return r.json()

    def delete(self, path: str, **kw) -> dict:
        r = self.s.delete(f"{self.base}{path}", **kw)
        self._raise_for_status(r)
        return r.json()

    # -- High-level helpers -------------------------------------------------

    def exec_cmd(
        self,
        tab_id: str,
        command: str,
        timeout_ms: int = 10000,
        strip_ansi_flag: bool = True,
    ) -> dict:
        """POST /api/tabs/{id}/exec — send command, wait for output.

        Returns dict with keys: output, offset, and optionally timeout.
        """
        return self.post(
            f"/api/tabs/{tab_id}/exec",
            json={
                "command": command,
                "timeout_ms": timeout_ms,
                "strip_ansi": strip_ansi_flag,
            },
        )

    def send_text(
        self,
        tab_id: str,
        text: str | None = None,
        enter: bool = True,
        batch: list[dict] | None = None,
    ) -> dict:
        """POST /api/tabs/{id}/send — send text / batch to PTY."""
        body: dict = {}
        if batch is not None:
            body["batch"] = batch
        else:
            if text is not None:
                body["text"] = text
            body["enter"] = enter
        return self.post(f"/api/tabs/{tab_id}/send", json=body)

    def get_output(
        self,
        tab_id: str,
        offset: int = 0,
        strip_ansi_flag: bool = False,
    ) -> dict:
        """GET /api/tabs/{id}/output — raw or stripped output buffer."""
        params: dict = {}
        if offset:
            params["offset"] = offset
        if strip_ansi_flag:
            params["strip_ansi"] = "true"
        return self.get(f"/api/tabs/{tab_id}/output", params=params)

    def wait_for_output(
        self,
        tab_id: str,
        pattern: str | None = None,
        timeout: float = 10,
        strip_ansi_flag: bool = True,
    ) -> str:
        """Poll GET /api/tabs/{id}/output until *pattern* appears or timeout.

        Returns the full stripped output text.  Raises TimeoutError on failure.
        """
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            data = self.get_output(tab_id, strip_ansi_flag=strip_ansi_flag)
            text = data.get("output", "")
            if pattern is None:
                return text
            if re.search(pattern, text):
                return text
            time.sleep(0.3)
        raise TimeoutError(
            f"Pattern {pattern!r} not found in tab {tab_id} output within {timeout}s"
        )

    def get_screen(self, tab_id: str) -> str:
        """GET /api/tabs/{id}/text — visible terminal text."""
        data = self.get(f"/api/tabs/{tab_id}/text")
        return data.get("text", "")

    # -- Workspace / Tab CRUD shortcuts ------------------------------------

    def create_workspace(self, name: str, cwd: str | None = None) -> dict:
        body: dict = {"name": name}
        if cwd is not None:
            body["cwd"] = cwd
        return self.post("/api/workspaces", json=body)

    def list_workspaces(self) -> list[dict]:
        return self.get("/api/workspaces")

    def rename_workspace(self, ws_id: str, name: str) -> dict:
        return self.post(f"/api/workspaces/{ws_id}/rename", json={"name": name})

    def close_workspace(self, ws_id: str) -> dict:
        return self.post(f"/api/workspaces/{ws_id}/close")

    def create_tab(self, ws_id: str, eager: bool = False) -> dict:
        body = {"eager": True} if eager else {}
        return self.post(f"/api/workspaces/{ws_id}/tabs", json=body)

    def list_tabs(self) -> list[dict]:
        return self.get("/api/tabs", params={"all_workspaces": "true"})

    def split_tab(self, tab_id: str, direction: str = "right") -> dict:
        return self.post(
            f"/api/tabs/{tab_id}/split", json={"direction": direction}
        )

    def close_tab(self, tab_id: str) -> dict:
        return self.post(f"/api/tabs/{tab_id}/close")

    def rename_tab(self, tab_id: str, title: str) -> dict:
        return self.post(f"/api/tabs/{tab_id}/rename", json={"title": title})

    def focus_tab(self, tab_id: str) -> dict:
        return self.post(f"/api/tabs/{tab_id}/focus")

    def wait_for_pty(self, tab_id: str, timeout: float = 20) -> bool:
        """Wait until a tab has a pty_id (PTY linked by frontend)."""
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            tabs = self.list_tabs()
            tab = next((p for p in tabs if p["id"] == tab_id), None)
            if tab and tab.get("pty_id"):
                return True
            time.sleep(0.5)
        return False

    def wait_for_screen(
        self,
        tab_id: str,
        pattern: str,
        timeout: float = 30,
    ) -> str:
        """Poll GET /api/tabs/{id}/text until pattern appears. Returns tab text."""
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                text = self.get_screen(tab_id)
                if re.search(pattern, text):
                    return text
            except Exception:
                pass
            time.sleep(1)
        raise TimeoutError(
            f"Pattern {pattern!r} not found on screen of tab {tab_id} within {timeout}s"
        )
