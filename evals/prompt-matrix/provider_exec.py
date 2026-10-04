"""WHAT: Run a qualified fixture-native provider with its exact launch scope.
WHY: Owner profiles and unverified executables must never become fixture fallbacks.
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
    from launch_env import isolated_environment, select_scope
    from native_assembly import load_native
    from native_layout import demand
    provider = options.provider
    active = select_scope(options.root, json.loads((options.root / "launch-scopes.json").read_text()),
                          provider, Path.cwd())
    demand(bool(os.environ.get("CODETERM_TAB_ID")), "Provider launch has no actual tab identity")
    demand(os.environ.get("CODETERM_API_PORT") == str(meta["port"]), "Provider transport port differs")
    manifest, executable = load_native(options.root, provider, active["version"])
    binary = str(executable)
    env = isolated_environment(os.environ, options.root, Path(active["home"]), meta["provider_tool_path"])
    if provider == "codex":
        env.update(CODEX_MANAGED_BY_NPM="1", CODEX_MANAGED_PACKAGE_ROOT=str(executable.parents[4] / "wrapper"))
    if active["case"] == "bad-model":
        args = model_args(args, provider, active["bad_model"])
    if active["case"].startswith("trust-") or active["case"] == "launch-prompt":
        args = [arg for arg in args if arg not in BYPASS]
    if active["case"] == "permission":
        args = permission_args(args, provider)
        if provider == "opencode":
            config = json.loads(env.get("OPENCODE_CONFIG_CONTENT", "{}"))
            config["permission"] = {"*": "ask"}
            env["OPENCODE_CONFIG_CONTENT"] = json.dumps(config)
    record = {
        "tab_id": env["CODETERM_TAB_ID"], "pid": os.getpid(), "ppid": os.getppid(),
        "provider": provider, "executable": binary, "cwd": os.getcwd(),
        "home": env["HOME"], "codex_home": env["CODEX_HOME"],
        "case": active["case"], "scope_id": active["scope_id"],
        "version_pin": active["version"], "native_sha256": manifest["executable_sha256"],
        "session": env.get("CODETERM_SESSION_ID"),
        "startup_attempt_id": env.get("CODETERM_STARTUP_ATTEMPT_ID"),
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
