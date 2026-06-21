#!/usr/bin/env python3
"""Capture full-session detection replays from scripted scenarios.

Dev-only: targets the CodeTerm dev daemon on port 7686. The recorded cast is the
deterministic artifact; the scenario is only the generator.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
import time
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any

try:
    import yaml
except ImportError:  # pragma: no cover - local dev dependency guard
    yaml = None


ROOT = Path(__file__).resolve().parents[1]
DEV_PORT = "7686"


def dev_env() -> dict[str, str]:
    cfg = Path.home() / ".codeterm-dev" / "config.toml"
    token = ""
    if cfg.exists():
        for line in cfg.read_text(encoding="utf-8").splitlines():
            if line.strip().startswith("token = "):
                token = line.split('"', 2)[1]
                break
    if not token:
        raise SystemExit(f"dev token not found in {cfg}")
    env = os.environ.copy()
    env.update(
        {
            "CODETERM_API_PORT": DEV_PORT,
            "CODETERM_API_TOKEN": token,
            "CODETERM_SOCKET_PATH": "codeterm-dev.sock",
            "CODETERM_PANE_ID": "",
            "CODETERM_PARENT_PANE_ID": "",
        }
    )
    return env


ENV = dev_env()


def dev_get(path: str) -> Any:
    req = urllib.request.Request(
        f"http://127.0.0.1:{DEV_PORT}{path}",
        headers={"Authorization": f"Bearer {ENV['CODETERM_API_TOKEN']}"},
    )
    with urllib.request.urlopen(req, timeout=10) as resp:
        return json.loads(resp.read().decode("utf-8"))


def dev_post(path: str, payload: dict[str, Any]) -> Any:
    req = urllib.request.Request(
        f"http://127.0.0.1:{DEV_PORT}{path}",
        data=json.dumps(payload).encode("utf-8"),
        headers={
            "Authorization": f"Bearer {ENV['CODETERM_API_TOKEN']}",
            "Content-Type": "application/json",
        },
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=10) as resp:
        return json.loads(resp.read().decode("utf-8"))


def resize_pane_for_capture(pane_id: str, cols: int, rows: int) -> dict[str, Any]:
    deadline = time.monotonic() + 20.0
    last_error: Exception | None = None
    while time.monotonic() < deadline:
        try:
            return dev_post(f"/api/panes/{pane_id}/resize", {"cols": cols, "rows": rows})
        except Exception as exc:
            last_error = exc
            time.sleep(0.75)
    raise RuntimeError(f"failed to resize replay pane {pane_id} to {cols}x{rows}: {last_error}")


def start_capture(pane_id: str, cast_path: Path) -> dict[str, Any]:
    deadline = time.monotonic() + 20.0
    last_error: Exception | None = None
    result: dict[str, Any] | None = None
    while time.monotonic() < deadline:
        try:
            result = dev_post(f"/api/panes/{pane_id}/capture/start", {"out": str(cast_path)})
            break
        except Exception as exc:
            last_error = exc
            time.sleep(0.5)
    if result is None:
        raise RuntimeError(f"failed to start capture for pane {pane_id}: {last_error}")
    deadline = time.monotonic() + 5.0
    while time.monotonic() < deadline:
        if cast_path.exists():
            return result
        time.sleep(0.1)
    raise RuntimeError(f"capture start returned ok but did not create {cast_path}")


def stop_capture(pane_id: str, cast_path: Path) -> dict[str, Any]:
    result = dev_post(f"/api/panes/{pane_id}/capture/stop", {})
    if not cast_path.exists():
        raise RuntimeError(f"capture stop returned ok but {cast_path} is missing")
    return result


def ensure_dev() -> None:
    try:
        dev_get("/api/health")
    except Exception as exc:
        raise SystemExit(
            "dev daemon is not healthy on port 7686; start it in a visible CodeTerm pane: "
            "codeterm pane new --cmd \"cd <repo> && ./build.sh dev\""
        ) from exc


def run_codeterm(*args: str, json_out: bool = False) -> Any:
    cmd = ["codeterm", *args]
    proc = subprocess.run(
        cmd,
        cwd=ROOT,
        env=ENV,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            f"{' '.join(cmd)} failed ({proc.returncode})\nSTDOUT:\n{proc.stdout}\nSTDERR:\n{proc.stderr}"
        )
    if json_out:
        return json.loads(proc.stdout)
    return proc.stdout.strip()


def pane_status(pane_id: str) -> str:
    try:
        return str(dev_get(f"/api/panes/{pane_id}/status").get("status", ""))
    except Exception:
        return ""


def pane_text(pane_id: str) -> str:
    try:
        return run_codeterm("pane", "get-text", "--pane", pane_id)
    except Exception:
        return ""


def normalize_for_stability(text: str) -> str:
    out: list[str] = []
    for line in text.splitlines():
        if is_structural_volatile_line(line):
            continue
        compact = " ".join(line.split())
        out.append(compact)
    return "\n".join(out).strip()


def is_structural_volatile_line(line: str) -> bool:
    low = line.lower()
    has_interrupt_hint = "esc to interrupt" in low or "esc interrupt" in low
    has_timer = bool(re.search(r"\b\d+\s*[hms]\b", line))
    has_spinner = bool(re.search(r"[✻✳✢✶✷✺✽●∗⋆⠁⠂⠄⠆⠇⠋⠙⠹⠸⠼⠴⠦⠧⠏]", line))
    has_token_meter = bool(re.search(r"[↑↓]\s*\d", line))
    return has_interrupt_hint or has_spinner or (has_timer and has_token_meter)


def active_terminal_work(text: str) -> bool:
    low = text.lower()
    if "esc to interrupt" in low or "esc interrupt" in low:
        return True
    return bool(re.search(r"…\s*\([^)]*\d+\s*s", text))


@dataclass
class StartupAction:
    label: str
    text: str | None = None
    actions: str | None = None


def startup_prompt_action(text: str) -> StartupAction | None:
    if composer_visible(text):
        return None
    if bracket_hotkey_trust_visible(text):
        return StartupAction("bracket_hotkey_trust", text="a", actions="enter")
    if numbered_menu_visible(text):
        return StartupAction("numbered_menu", actions="enter")
    return None


def composer_visible(text: str) -> bool:
    return any(re.match(r"^\s*(❯|›|→)\s*", line) for line in text.splitlines())


def numbered_menu_visible(text: str) -> bool:
    option_rows = 0
    selected = False
    for line in text.splitlines()[-16:]:
        if re.match(r"^\s*[❯›>]?\s*\d+\.\s+\S", line):
            option_rows += 1
            selected = selected or bool(re.match(r"^\s*[❯›>]", line))
    return option_rows >= 2 or (option_rows >= 1 and selected)


def bracket_hotkey_trust_visible(text: str) -> bool:
    rows = [line.strip() for line in text.splitlines()[-24:]]
    has_accept = any(re.match(r"^\[[aA]\]\s+\S", row) for row in rows)
    has_decline = any(re.match(r"^\[[qQ]\]\s+\S", row) for row in rows)
    has_box = any(row.startswith(("╭", "│", "╰")) for row in rows)
    return has_box and has_accept and has_decline


def shell_prompt_visible(text: str) -> bool:
    return any(re.match(r"^\S.*[%$]\s*$", line) for line in text.splitlines()[-6:])


@dataclass
class WaitResult:
    state: str
    elapsed_ms: int
    stable_ms: int
    timed_out: bool


def wait_stable(pane_id: str, pacing: dict[str, Any]) -> WaitResult:
    start = time.monotonic()
    max_wait = float(pacing.get("max_wait_ms", 180_000)) / 1000.0
    min_dwell = float(pacing.get("min_dwell_ms", 1200)) / 1000.0
    quiet = float(pacing.get("stability_quiet_ms", 2500)) / 1000.0
    last_norm = ""
    stable_since = time.monotonic()
    last_status = ""

    while True:
        now = time.monotonic()
        text = pane_text(pane_id)
        norm = normalize_for_stability(text)
        status = pane_status(pane_id)
        last_status = status or last_status
        if norm != last_norm:
            last_norm = norm
            stable_since = now
        stable_for = now - stable_since
        elapsed = now - start
        terminal_active = active_terminal_work(text)
        status_ready = status in {"", "Idle", "Waiting"}
        if elapsed >= min_dwell and stable_for >= quiet and (status_ready or not terminal_active):
            return WaitResult(last_status or "stable", int(elapsed * 1000), int(stable_for * 1000), False)
        if elapsed >= max_wait:
            return WaitResult(last_status or "timeout", int(elapsed * 1000), int(stable_for * 1000), True)
        time.sleep(1.0)


def settle_startup_prompts(pane_id: str, log: list[dict[str, Any]], rel_ms) -> None:
    deadline = time.monotonic() + 25.0
    while time.monotonic() < deadline:
        action = startup_prompt_action(pane_text(pane_id))
        if action is None:
            return
        args = ["send"]
        if action.text is not None:
            args.append(action.text)
        if action.actions is not None:
            args.extend(["--actions", action.actions])
        args.extend(["--pane", pane_id])
        run_codeterm(*args)
        log.append(
            {
                "t_ms": rel_ms(),
                "event": "startup_prompt_answered",
                "action": action.label,
                "text": action.text,
                "actions": action.actions,
            }
        )
        time.sleep(2.0)


def recover_agent_after_startup(pane_id: str, provider: str, log: list[dict[str, Any]], rel_ms) -> None:
    text = pane_text(pane_id)
    if provider != "cursor" or composer_visible(text) or not shell_prompt_visible(text):
        return
    run_codeterm("agent", "restart", "--pane", pane_id, "--force")
    log.append({"t_ms": rel_ms(), "event": "startup_agent_restart", "provider": provider})
    time.sleep(8.0)
    settle_startup_prompts(pane_id, log, rel_ms)


def provider_version(provider: str) -> str:
    bins = {
        "claude": ["claude"],
        "codex": ["codex"],
        "cursor": ["cursor-agent", "cursor"],
        "opencode": ["opencode"],
    }.get(provider, [provider])
    for bin_name in bins:
        try:
            proc = subprocess.run(
                [bin_name, "--version"],
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                check=False,
            )
            if proc.stdout.strip():
                return proc.stdout.splitlines()[0]
        except FileNotFoundError:
            continue
    return "unavailable"


def load_scenario(path: Path) -> dict[str, Any]:
    if yaml is None:
        raise SystemExit("PyYAML is required to read scenario YAML files")
    with path.open(encoding="utf-8") as f:
        return yaml.safe_load(f)


def write_json(path: Path, data: Any) -> None:
    path.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def capture(args: argparse.Namespace) -> Path:
    scenario = load_scenario(args.scenario)
    provider = args.provider or scenario.get("first_capture") or scenario["providers"][0]
    terminal_cfg = scenario.get("workspace_setup", {}).get("terminal", {})
    cols = int(args.cols or terminal_cfg.get("cols", 120))
    rows = int(args.rows or terminal_cfg.get("rows", 32))
    run_id = args.run_id or time.strftime("%Y%m%d-%H%M%S")
    out_dir = Path(args.out_root) / provider / f"{scenario['id']}-{run_id}"
    if args.dry_run:
        print(f"scenario={scenario['id']} provider={provider} turns={len(scenario['turns'])}")
        for turn in scenario["turns"]:
            expect = turn.get("expect", {})
            print(f"{turn['n']:02d}. {turn['user']}  expect={expect}")
        return out_dir

    ensure_dev()
    out_dir.mkdir(parents=True, exist_ok=True)

    workspace_dir = Path(tempfile.mkdtemp(prefix=f"ct-replay-{provider}-{scenario['id']}-"))
    ws = run_codeterm(
        "workspace",
        "create",
        f"replay-{provider}-{scenario['id']}-{run_id}",
        "--cwd",
        str(workspace_dir),
        "--json",
        json_out=True,
    )
    workspace_id = ws["id"]
    pane = run_codeterm(
        "agent",
        "spawn",
        provider,
        "--workspace",
        workspace_id,
        "--cwd",
        str(workspace_dir),
        "--title",
        f"replay-{scenario['id']}",
        "--json",
        json_out=True,
    )
    pane_id = pane["pane_id"]

    events: list[dict[str, Any]] = []
    phases: list[dict[str, Any]] = []
    log: list[dict[str, Any]] = []
    start_wall_ms = int(time.time() * 1000)
    cast_path = out_dir / "raw.cast"

    def rel_ms() -> int:
        return int(time.time() * 1000) - start_wall_ms

    capture_start_result = start_capture(pane_id, cast_path)
    start_wall_ms = int(time.time() * 1000)
    log.append(
        {
            "t_ms": rel_ms(),
            "event": "capture_start",
            "cast": str(cast_path),
            "capture_start_result": capture_start_result,
        }
    )
    resize_result = resize_pane_for_capture(pane_id, cols, rows)
    log.append(
        {
            "t_ms": rel_ms(),
            "event": "capture_resize",
            "terminal": {"cols": cols, "rows": rows},
            "resize_result": resize_result,
        }
    )
    time.sleep(0.5)

    focus_peer_pane_id = ""
    focus_churn = scenario.get("focus_churn", {})
    if focus_churn.get("enabled"):
        try:
            peer = run_codeterm(
                "pane",
                "new",
                "--workspace",
                workspace_id,
                "--json",
                json_out=True,
            )
            focus_peer_pane_id = peer.get("pane_id", "")
        except Exception as exc:
            focus_peer_pane_id = ""
            print(f"warning: focus peer unavailable; focus churn disabled: {exc}", file=sys.stderr)

    def flush_progress() -> None:
        write_json(out_dir / "phases.json", {"events": events, "phases": phases, "oracle": scenario.get("oracle", {})})
        write_json(out_dir / "capture.log.json", log)

    def churn_focus(label: str, cycles: int | None = None, delay_ms: int | None = None) -> None:
        if not focus_peer_pane_id:
            return
        cycles = cycles if cycles is not None else int(focus_churn.get("cycles", 6))
        delay_ms = delay_ms if delay_ms is not None else int(focus_churn.get("delay_ms", 350))
        for i in range(cycles):
            run_codeterm("pane", "focus", focus_peer_pane_id)
            time.sleep(delay_ms / 1000.0)
            run_codeterm("pane", "focus", pane_id)
            time.sleep(delay_ms / 1000.0)
            log.append({"t_ms": rel_ms(), "event": "focus_churn", "label": label, "cycle": i + 1})

    time.sleep(5)
    log.append(
        {
            "t_ms": rel_ms(),
            "event": "spawned",
            "pane": pane_id,
            "workspace": workspace_id,
            "terminal": {"cols": cols, "rows": rows},
            "resize_result": resize_result,
        }
    )
    settle_startup_prompts(pane_id, log, rel_ms)
    recover_agent_after_startup(pane_id, provider, log, rel_ms)
    wait_stable(pane_id, scenario.get("pacing", {}))

    for turn in scenario["turns"]:
        before = rel_ms()
        text = turn["user"]
        run_codeterm("send", "--pane", pane_id, text, "--actions", "enter")
        events.append({"t_ms": before, "kind": "input", "turn": turn["n"], "text": text})
        log.append({"t_ms": before, "event": "send_turn", "turn": turn["n"]})
        flush_progress()
        wait = wait_stable(pane_id, scenario.get("pacing", {}))
        after = rel_ms()
        phases.append(
            {
                "from_ms": before,
                "to_ms": after,
                "turn": turn["n"],
                "expected": turn.get("expect", {}),
                "pacing": {
                    "state": wait.state,
                    "elapsed_ms": wait.elapsed_ms,
                    "stable_ms": wait.stable_ms,
                    "timed_out": wait.timed_out,
                },
            }
        )
        log.append(
            {
                "t_ms": after,
                "event": "turn_settled",
                "turn": turn["n"],
                "wait": wait.__dict__,
            }
        )
        flush_progress()
        if turn.get("idle_after_ms"):
            idle_start = rel_ms()
            duration_ms = int(turn["idle_after_ms"])
            events.append({"t_ms": idle_start, "kind": "idle_hold", "turn": turn["n"], "duration_ms": duration_ms})
            log.append({"t_ms": idle_start, "event": "idle_hold_start", "turn": turn["n"], "duration_ms": duration_ms})
            churn = turn.get("focus_churn", {})
            if churn.get("enabled", focus_churn.get("enabled", False)):
                churn_focus(
                    f"turn-{turn['n']}",
                    cycles=int(churn.get("cycles", focus_churn.get("cycles", 6))),
                    delay_ms=int(churn.get("delay_ms", focus_churn.get("delay_ms", 350))),
                )
            remaining = duration_ms - (rel_ms() - idle_start)
            if remaining > 0:
                time.sleep(remaining / 1000.0)
            phases.append(
                {
                    "from_ms": idle_start,
                    "to_ms": rel_ms(),
                    "turn": turn["n"],
                    "expected": {"phases": ["idle"], "card": "none", "negative": "idle_hold"},
                    "pacing": {"state": "Idle", "elapsed_ms": duration_ms, "stable_ms": duration_ms, "timed_out": False},
                }
            )
            flush_progress()

    final_idle_ms = int(scenario.get("stop", {}).get("after_final_idle_ms", 3000))
    final_start = rel_ms()
    if scenario.get("stop", {}).get("focus_churn"):
        churn_focus("final-idle")
    time.sleep(max(0, final_idle_ms - (rel_ms() - final_start)) / 1000.0)
    capture_stop_result = stop_capture(pane_id, cast_path)
    log.append({"t_ms": rel_ms(), "event": "capture_stop", "capture_stop_result": capture_stop_result})
    screen = pane_text(pane_id)
    (out_dir / "screen.txt").write_text(screen, encoding="utf-8")

    meta = {
        "scenario": scenario["id"],
        "title": scenario.get("title"),
        "provider": provider,
        "provider_version": provider_version(provider),
        "codeterm_health": dev_get("/api/health"),
        "workspace_id": workspace_id,
        "pane_id": pane_id,
        "workspace_dir": str(workspace_dir),
        "captured_at_ms": int(time.time() * 1000),
        "terminal": {
            "cols": cols,
            "rows": rows,
            "scenario_default": terminal_cfg,
            "resize_result": resize_result,
        },
        "source": "testbench/replay-capture.py",
    }
    write_json(out_dir / "meta.json", meta)
    write_json(out_dir / "capture.log.json", log)
    write_json(out_dir / "scenario.json", scenario)
    print(out_dir)
    return out_dir


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("scenario", type=Path)
    parser.add_argument("--provider")
    parser.add_argument("--run-id")
    parser.add_argument("--out-root", default=str(ROOT / "testbench" / "replays"))
    parser.add_argument("--cols", type=int)
    parser.add_argument("--rows", type=int)
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    capture(args)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
