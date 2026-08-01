import json
import os
import shutil
import subprocess
import time

from helpers import unique_name


def socket_env(api) -> dict[str, str]:
    env = os.environ.copy()
    env["CODETERM_API_PORT"] = "7686"
    env["CODETERM_API_TOKEN"] = api.s.headers["Authorization"].removeprefix("Bearer ")
    env["CODETERM_SOCKET_PATH"] = r"\\.\pipe\codeterm-dev.sock" if os.name == "nt" else "codeterm-dev.sock"
    env["CODETERM_TAB_ID"] = ""
    env["CODETERM_PARENT_TAB_ID"] = ""
    return env


def run_socket_cli(args: list[str], env: dict[str, str], timeout: float) -> subprocess.CompletedProcess[str]:
    exe = shutil.which("codeterm")
    assert exe, "codeterm CLI must be on PATH for socket e2e"
    return subprocess.run(
        [exe, *args],
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout,
        check=True,
    )


def test_socket_eager_create_then_immediate_list_returns(api_only, tmp_path):
    env = socket_env(api_only)
    ws_id = None
    name = unique_name("e2e-ws-socket")
    cwd = str(tmp_path)
    try:
        created = run_socket_cli(
            ["workspace", "create", name, "--cwd", cwd, "--json"],
            env,
            timeout=10,
        )
        ws = json.loads(created.stdout)
        ws_id = ws["id"]

        run_socket_cli(
            [
                "tab",
                "new",
                "--workspace",
                ws_id,
                "--cwd",
                cwd,
                "--title",
                "socket-eager",
                "--eager",
                "--json",
            ],
            env,
            timeout=10,
        )

        started = time.monotonic()
        listed = run_socket_cli(["workspace", "list", "--json"], env, timeout=2)
        elapsed = time.monotonic() - started
        assert elapsed < 2
        assert any(w["id"] == ws_id for w in json.loads(listed.stdout))
    finally:
        if ws_id:
            api_only.close_workspace(ws_id)
