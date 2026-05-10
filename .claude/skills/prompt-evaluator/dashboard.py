"""Tiny in-process dashboard for prompt-evaluator runs.

Pure stdlib (http.server + threading). No external deps.

Usage from runner.py:
    from dashboard import Dashboard
    dash = Dashboard(port=7779, total_calls=588)
    dash.start()  # spawns daemon thread, opens browser
    dash.event("eval_started", eval_id=1, eval_name="...", prompt="...")
    dash.event("run_done", eval_id=1, run_idx=0, passed=True, response="...")
    dash.event("eval_done", eval_id=1, runs_passed=3, total_runs=3)
    dash.set_summary(...)
    # served at http://localhost:7779
    dash.wait_for_close()  # optional — block until user opens /shutdown
"""
from __future__ import annotations

import json
import threading
import webbrowser
from collections import deque
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any


HERE = Path(__file__).parent
HTML_PATH = HERE / "dashboard.html"


class _State:
    """Shared mutable state — guarded by a single lock."""

    def __init__(self, total_calls: int, ab_mode: bool, prompt_label: str) -> None:
        self.lock = threading.Lock()
        self.events: deque[dict[str, Any]] = deque(maxlen=2000)
        self.evals: dict[int, dict[str, Any]] = {}  # eval_id -> agg
        self.started_at = datetime.now(timezone.utc).isoformat()
        self.finished_at: str | None = None
        self.total_calls = total_calls
        self.calls_done = 0
        self.ab_mode = ab_mode
        self.prompt_label = prompt_label
        self.report_md: str | None = None

    def append_event(self, etype: str, **kw: Any) -> None:
        with self.lock:
            ev = {
                "type": etype,
                "ts": datetime.now(timezone.utc).isoformat(),
                **kw,
            }
            self.events.append(ev)

            if etype == "call_done":
                self.calls_done += 1
            elif etype == "eval_started":
                self.evals[kw["eval_id"]] = {
                    "id": kw["eval_id"],
                    "name": kw["eval_name"],
                    "prompt": kw.get("prompt", ""),
                    "expected": kw.get("expected", ""),
                    "expectations": kw.get("expectations", []),
                    "side": kw.get("side"),  # "A" / "B" / None
                    "runs": [],
                    "status": "running",
                }
            elif etype == "run_done":
                ev_state = self.evals.setdefault(kw["eval_id"], {
                    "id": kw["eval_id"],
                    "name": kw.get("eval_name", "?"),
                    "runs": [],
                    "status": "running",
                })
                ev_state["runs"].append({
                    "run_idx": kw["run_idx"],
                    "passed": kw["passed"],
                    "response": kw.get("response", "")[:1500],
                    "matches": kw.get("matches", []),
                    "side": kw.get("side"),
                })
            elif etype == "eval_done":
                ev_state = self.evals.setdefault(kw["eval_id"], {})
                ev_state["status"] = "done"
                ev_state["runs_passed"] = kw["runs_passed"]
                ev_state["total_runs"] = kw["total_runs"]
            elif etype == "finished":
                self.finished_at = datetime.now(timezone.utc).isoformat()
                self.report_md = kw.get("report_md")

    def snapshot(self) -> dict[str, Any]:
        with self.lock:
            return {
                "started_at": self.started_at,
                "finished_at": self.finished_at,
                "total_calls": self.total_calls,
                "calls_done": self.calls_done,
                "ab_mode": self.ab_mode,
                "prompt_label": self.prompt_label,
                "evals": list(self.evals.values()),
                "report_md": self.report_md,
            }


class _Handler(BaseHTTPRequestHandler):
    state: _State  # set on the class by Dashboard.start

    def log_message(self, format: str, *args: Any) -> None:  # noqa: A003
        return  # silence stdlib chatter

    def _send_json(self, payload: Any, status: int = 200) -> None:
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-cache")
        self.end_headers()
        self.wfile.write(body)

    def _send_html(self, body: bytes) -> None:
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-cache")
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self) -> None:  # noqa: N802
        if self.path in ("/", "/index.html"):
            try:
                body = HTML_PATH.read_bytes()
            except OSError:
                body = b"<h1>dashboard.html missing</h1>"
            self._send_html(body)
            return
        if self.path == "/state.json":
            self._send_json(self.state.snapshot())
            return
        if self.path == "/health":
            self._send_json({"ok": True})
            return
        self.send_response(404)
        self.end_headers()


class Dashboard:
    def __init__(
        self,
        port: int,
        total_calls: int,
        ab_mode: bool = False,
        prompt_label: str = "",
        open_browser: bool = True,
    ) -> None:
        self.port = port
        self.state = _State(total_calls, ab_mode, prompt_label)
        self._open_browser = open_browser
        self._server: ThreadingHTTPServer | None = None
        self._thread: threading.Thread | None = None

    def start(self) -> None:
        # Bind a per-instance handler class so multiple dashboards don't clash.
        Handler = type("Handler", (_Handler,), {"state": self.state})
        self._server = ThreadingHTTPServer(("127.0.0.1", self.port), Handler)
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)
        self._thread.start()
        url = f"http://127.0.0.1:{self.port}"
        print(f"[dashboard] live at {url}", flush=True)
        if self._open_browser:
            try:
                webbrowser.open(url)
            except Exception:
                pass

    def event(self, etype: str, **kw: Any) -> None:
        self.state.append_event(etype, **kw)

    def stop(self) -> None:
        if self._server is not None:
            self._server.shutdown()
            self._server.server_close()
