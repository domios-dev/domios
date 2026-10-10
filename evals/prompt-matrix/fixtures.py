"""WHAT: Prepare isolated instance state and provider launch environments.
WHY: Every provider and observer must use fixture-owned homes and verified binaries.
ORACLE: Every launch records its tab, PID, executable and selected home paths.
"""

import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys


PROVIDERS = ("claude", "codex", "opencode", "cursor")
HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(value, indent=2) + "\n")
    temporary.chmod(0o600)
    temporary.replace(path)


def canonical():
    common = subprocess.check_output(
        ["git", "rev-parse", "--git-common-dir"], cwd=REPO, text=True, timeout=10
    ).strip()
    root = (REPO / common).resolve().parent
    if REPO != root:
        raise RuntimeError(f"Live gates require canonical checkout {root}; current {REPO}")


def fixture_root(instance, port):
    if not re.fullmatch(r"[a-z][a-z0-9-]{2,39}", instance):
        raise RuntimeError("An explicit lowercase named instance is required")
    if port in (7685, 7686) or not 1024 <= port <= 65535:
        raise RuntimeError("Production and shared dev ports are forbidden")
    return REPO / "target-check" / "evals" / "prompt-matrix" / instance


def prepare(instance, port):
    canonical()
    root = fixture_root(instance, port)
    if root.exists():
        raise RuntimeError(f"Fixture already exists: {root}; inspect/retain its evidence first")
    root.mkdir(parents=True, mode=0o700)
    home = root / "daemon-home"
    data = home / f".codeterm-dev-{instance}"
    data.mkdir(parents=True, mode=0o700)
    (data / "config.toml").write_text(
        "[general_agent]\nenabled = false\n"
        "[memory]\nfile_sync_enabled = false\nlibrarian_enabled = false\n"
        "[updater]\nallow_provider_updates_in_non_prod = false\n"
    )
    binary_names = {}
    for provider in PROVIDERS:
        manifest = json.loads((REPO / "plugins" / f"provider-{provider}" / "plugin.json").read_text())
        binary_names[provider] = manifest["cliBinary"]
    metadata = {
        "instance": instance, "port": port, "root": str(root), "binary_names": binary_names,
        "build_tool_path": os.environ["PATH"],
        "cargo_home": os.environ.get("CARGO_HOME", str(Path.home() / ".cargo")),
        "rustup_home": os.environ.get("RUSTUP_HOME", str(Path.home() / ".rustup")),
        "provider_tool_path": "",
        "auth_status": "unqualified",
    }
    write_json(root / "instance.json", metadata)
    bin_dir = root / "bin"
    bin_dir.mkdir()
    for provider in PROVIDERS:
        name = binary_names[provider]
        wrapper = bin_dir / name
        wrapper.write_text(
            "#!/usr/bin/env bash\nset -euo pipefail\nexec "
            + shlex.quote(sys.executable) + " " + shlex.quote(str(HERE / "provider_exec.py"))
            + " --root " + shlex.quote(str(root)) + " --provider "
            + shlex.quote(provider) + ' -- "$@"\n'
        )
        wrapper.chmod(0o700)
        windows_wrapper = bin_dir / (name + ".cmd")
        windows_wrapper.write_text(
            '@echo off\n"' + sys.executable + '" "' + str(HERE / "provider_exec.py")
            + '" --root "' + str(root) + '" --provider ' + provider + ' -- %*\n'
        )
    return root


def environment(root):
    meta = json.loads((root / "instance.json").read_text())
    from launch_env import isolated_environment, make_home
    home = make_home(root, root / "daemon-home")
    env = isolated_environment(os.environ, root, home, str(root / "bin") + os.pathsep + meta["build_tool_path"])
    for name in tuple(env):
        if name.startswith("CODETERM_"):
            del env[name]
    env.update({"CODETERM_INSTANCE": meta["instance"], "CODETERM_API_PORT": str(meta["port"]),
                "CARGO_HOME": meta["cargo_home"], "RUSTUP_HOME": meta["rustup_home"]})
    return env


def case_fixture(root, provider, case, variant, bad_model):
    import tempfile
    case_root = Path(tempfile.mkdtemp(prefix=f"{provider}-{case}-", dir=root))
    from launch_env import make_home
    from uuid import uuid4
    home = make_home(root, case_root / "home")
    cwd = case_root / "owner-directory"
    for path in (home, cwd, home / ".codex", home / ".claude"):
        path.mkdir(parents=True, exist_ok=True)
    config = {
        "provider": provider, "case": case, "variant": variant,
        "case_root": str(case_root), "home": str(home), "cwd": str(cwd),
        "bad_model": bad_model, "scope_id": str(uuid4()),
        "version": json.loads((HERE / "versions.json").read_text())["providers"].get(provider, {}).get("target"),
    }
    return config
