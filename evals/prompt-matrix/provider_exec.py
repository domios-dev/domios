"""WHAT: Run an installed real provider with a case-specific environment.
WHY: Preserve real auth except for login; scope bad models and older binaries.
ORACLE: A per-launch record binds the real executable and PID to the dev tab.
"""

import argparse
import json
import os
from pathlib import Path
import sys

from fixtures import write_json


BYPASS = {
    "--dangerously-bypass-approvals-and-sandbox", "--dangerously-bypass-hook-trust",
    "--dangerously-skip-permissions", "--yolo", "--force", "--full-auto",
}


def model_args(args, provider, model):
    flag = "--model" if provider in ("claude", "cursor") else "-m"
    result = []
    skip = False
    for arg in args:
        if skip:
            skip = False
            continue
        if arg in ("-m", "--model"):
            skip = True
        elif not arg.startswith("--model="):
            result.append(arg)
    if skip:
        raise RuntimeError("Provider model flag has no argument")
    return result + [flag, model]


def permission_args(args, provider):
    result = []
    skip = False
    for arg in args:
        if skip:
            skip = False
            continue
        if arg in ("--ask-for-approval", "-a", "--sandbox", "-s", "--permission-mode"):
            skip = True
        elif arg not in BYPASS:
            result.append(arg)
    if provider == "codex":
        result += ["--ask-for-approval", "untrusted", "--sandbox", "workspace-write"]
    if provider == "claude":
        result += ["--permission-mode", "default"]
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--provider", required=True)
    parser.add_argument("args", nargs=argparse.REMAINDER)
    options = parser.parse_args()
    args = options.args[1:] if options.args[:1] == ["--"] else options.args
    meta = json.loads((options.root / "instance.json").read_text())
    active = json.loads((options.root / "active.json").read_text()) if (
        options.root / "active.json"
    ).exists() else {}
    provider = options.provider
    binary = meta["binaries"][provider]
    if not binary:
        raise RuntimeError(f"Provider binary absent: {provider}")
    env = dict(os.environ)
    is_tab = bool(env.get("CODETERM_TAB_ID"))
    testing = is_tab and active.get("provider") == provider
    if is_tab:
        env["HOME"] = meta["owner_home"]
        env["CODEX_HOME"] = meta["owner_codex_home"]
        env["CLAUDE_CONFIG_DIR"] = meta["owner_claude_config"]
        env["XDG_CONFIG_HOME"] = meta["owner_xdg_config"]
        env["XDG_DATA_HOME"] = meta["owner_xdg_data"]
    if testing and active["case"] == "login":
        home = Path(active["home"])
        env.update({
            "HOME": str(home), "CODEX_HOME": str(home / ".codex"),
            "CLAUDE_CONFIG_DIR": str(home / ".claude"),
            "XDG_CONFIG_HOME": str(home / ".config"),
            "XDG_DATA_HOME": str(home / ".local/share"),
        })
        for name in tuple(env):
            if any(word in name for word in ("API_KEY", "OAUTH_TOKEN", "AUTH_TOKEN")):
                del env[name]
    if testing and active["case"] == "bad-model":
        args = model_args(args, provider, active["bad_model"])
    if testing and active["case"].startswith("trust-"):
        args = [arg for arg in args if arg not in BYPASS]
    if testing and active["case"] == "permission":
        args = permission_args(args, provider)
        if provider == "opencode":
            config = json.loads(env.get("OPENCODE_CONFIG_CONTENT", "{}"))
            config["permission"] = {"*": "ask"}
            env["OPENCODE_CONFIG_CONTENT"] = json.dumps(config)
    if active.get("provider") == provider and active.get("case") in ("update", "update-coordinator"):
        older = Path(active["prefix"]) / "bin" / meta["binary_names"][provider]
        if not older.is_file():
            raise RuntimeError("Isolated older provider prefix has not been installed")
        binary = str(older)
        env["PATH"] = str(older.parent) + os.pathsep + env["PATH"]
        if active.get("variant") == "fail":
            env["npm_config_registry"] = "http://127.0.0.1:1"
            env["npm_config_fetch_retries"] = "0"
            env["npm_config_fetch_timeout"] = "3000"
    if testing:
        record = {
            "tab_id": env["CODETERM_TAB_ID"], "pid": os.getpid(), "ppid": os.getppid(),
            "provider": provider, "executable": binary, "cwd": os.getcwd(),
            "home": env["HOME"], "codex_home": env["CODEX_HOME"],
            "case": active["case"],
            "model_rewritten": active["case"] == "bad-model",
            "permission_default": active["case"] == "permission",
        }
        write_json(Path(active["case_root"]) / f"launch-{os.getpid()}.json", record)
    os.execve(binary, [binary, *args], env)


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"prompt-matrix provider launch refused: {error}", file=sys.stderr)
        sys.exit(7)
