"""Fixture homes and exact transport identity; no inherited provider credentials."""

import json
import os
from pathlib import Path

from native_archive import within
from native_layout import demand

TRANSPORT = {"CODETERM_API_URL", "CODETERM_API_PORT", "CODETERM_API_TOKEN", "CODETERM_SOCKET_PATH",
             "CODETERM_TAB_ID", "CODETERM_PARENT_TAB_ID", "CODETERM_SESSION_ID",
             "CODETERM_SESSION_BINDING_NONCE", "CODETERM_DEV_DATA_DIR", "CODETERM_INSTANCE",
             "CODETERM_WORKER_ID", "CODETERM_STARTUP_ATTEMPT_ID"}
SYSTEM = {"LANG", "LC_ALL", "LC_CTYPE", "TERM", "COLORTERM", "SYSTEMROOT", "WINDIR", "COMSPEC", "PATHEXT"}


def isolated_environment(parent, root, home, tool_path=""):
    home = within(root, home)
    env = {key: value for key, value in parent.items() if key in SYSTEM or key in TRANSPORT}
    env.update({"HOME": str(home), "USERPROFILE": str(home), "CODEX_HOME": str(home / ".codex"),
                "CLAUDE_CONFIG_DIR": str(home / ".claude"), "XDG_CONFIG_HOME": str(home / ".config"),
                "XDG_DATA_HOME": str(home / ".local/share"), "XDG_STATE_HOME": str(home / ".local/state"),
                "XDG_CACHE_HOME": str(home / ".cache"), "APPDATA": str(home / "AppData/Roaming"),
                "LOCALAPPDATA": str(home / "AppData/Local"), "TMPDIR": str(home / "tmp"),
                "TMP": str(home / "tmp"), "TEMP": str(home / "tmp"), "PATH": tool_path,
                "npm_config_prefix": str(home / "npm"), "npm_config_cache": str(home / "npm-cache"),
                "npm_config_userconfig": str(home / "empty-npmrc"),
                "npm_config_globalconfig": str(home / "empty-npmrc")})
    return env


def make_home(root, home):
    home = within(root, home)
    for suffix in (".codex", ".claude", ".config", ".local/share", ".local/state", ".cache",
                   "AppData/Roaming", "AppData/Local", "tmp", "npm", "npm-cache"):
        (home / suffix).mkdir(parents=True, exist_ok=True, mode=0o700)
    npmrc = within(home, home / "empty-npmrc")
    demand(not npmrc.exists() or npmrc.stat().st_size == 0, "Fixture empty npm config changed")
    npmrc.touch(mode=0o600, exist_ok=True)
    return home


def register_scope(root, fixture, cwd):
    from fixtures import write_json
    cwd = within(root, cwd)
    demand(cwd.is_dir(), "Launch scope cwd does not exist")
    path = root / "launch-scopes.json"
    rows = json.loads(path.read_text()) if path.exists() else []
    demand(not any(row["provider"] == fixture["provider"] and row["cwd"] == str(cwd) for row in rows),
           "Duplicate launch scope cwd")
    scope = {key: fixture[key] for key in ("scope_id", "provider", "case", "variant", "case_root",
                                          "home", "version", "bad_model")}
    scope["cwd"] = str(cwd)
    rows.append(scope)
    write_json(path, rows)
    return scope


def select_scope(root, rows, provider, cwd):
    cwd = within(root, cwd)
    matches = [row for row in rows if row["provider"] == provider and row["cwd"] == str(cwd)]
    demand(len(matches) == 1, "Provider launch has no unique exact fixture scope")
    scope = matches[0]
    case_root = within(root, scope["case_root"])
    within(case_root, scope["home"])
    demand(bool(scope["scope_id"]), "Launch scope identity is absent")
    return scope


def remove_scope(root, scope_id):
    from fixtures import write_json
    path = root / "launch-scopes.json"
    if path.exists():
        write_json(path, [row for row in json.loads(path.read_text()) if row["scope_id"] != scope_id])
