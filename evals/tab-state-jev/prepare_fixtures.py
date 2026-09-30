#!/usr/bin/env python3
"""Assemble the checked-in screen set from repository test fixtures."""

from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
OUT = Path(__file__).with_name("screens.jsonl")
LIVE_INPUT = Path(__file__).with_name("live-captures.jsonl")
LABELS = {
    "ready_idle",
    "working_normally",
    "provider_update_prompt",
    "trust_or_permission_prompt",
    "login_or_auth_required",
    "model_unavailable_or_quota",
    "crashed_to_shell",
    "error_loop",
    "still_starting",
}
TAB_STATE_SOURCE = {
    "codex": "plugins/provider-codex/tests/classify-tab-state.test.ts",
    "claude": "plugins/provider-claude/tests/classify-tab-state.test.ts",
}


def regex_scrub(screen: str) -> str:
    screen = re.sub(
        r"(?i)(?:[A-Z]:\\Users\\[^\\\s]+(?:\\[^\s]*)*|[A-Z]:\\Developer\\[^\s]*|"
        r"~/Developer/[^\s]*|~\\[^\\\s]+(?:\\[^\s]*)*|~/\.codeterm-dev[^\s]*|"
        r"~\\\.codeterm-dev[^\s]*|/(?:home|Users)/[^\s]+|/srv/(?:app|codeterm)[^\s]*)",
        "<workspace>",
        screen,
    )
    screen = re.sub(
        r"(?i)(?:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|"
        r"\btab=[0-9a-f]{8}\b|\bTab [0-9a-f]{8}\b)",
        "<id>",
        screen,
    )
    screen = re.sub(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}", "<email>", screen)
    screen = screen.replace("user@host", "agent@host")
    screen = screen.replace("C:\\Users\\foo", "C:\\Users\\agent")
    screen = screen.replace("C:\\Users\\User", "C:\\Users\\agent")
    return screen


def provider_tab_baseline(provider: str, raw: str) -> dict[str, Any]:
    mapped = {
        "update_prompt": "provider_update_prompt",
        "clarifying_question": "trust_or_permission_prompt",
        "waiting_on_auth": "login_or_auth_required",
        "rate_limited": "model_unavailable_or_quota",
    }[raw]
    return {
        "provider_tab_state": {
            "raw": raw,
            "label": mapped,
            "source": TAB_STATE_SOURCE[provider],
        }
    }


def add_fixture(
    rows: list[dict[str, Any]],
    provider: str,
    source: str,
    label: str,
    *,
    tab_state: str | None = None,
    directory_trust_response: str | None = None,
    core_prompt_kind: str | None = None,
    core_prompt_source: str | None = None,
    boot_stage: str | None = None,
) -> None:
    if label not in LABELS:
        raise ValueError(f"unsupported label for {source}: {label}")
    path = ROOT / source
    screen = path.read_text(encoding="utf-8").replace("\r\n", "\n").replace("\r", "\n")
    screen = "\n".join(screen.splitlines()[-40:])
    screen = regex_scrub(screen)
    baselines: dict[str, Any] = {}
    if tab_state:
        baselines.update(provider_tab_baseline(provider, tab_state))
    if directory_trust_response is not None:
        baselines["directory_trust_parser"] = {
            "raw": directory_trust_response,
            "label": "trust_or_permission_prompt"
            if directory_trust_response != "none"
            else None,
            "source": "plugins/provider-codex/tests/directory-trust-frames.test.ts"
            if provider == "codex"
            else "plugins/provider-claude/tests/plugin.test.ts",
        }
    if core_prompt_kind:
        mapped = {
            "ManualUpdate": "provider_update_prompt",
            "DirectoryTrust": "trust_or_permission_prompt",
        }[core_prompt_kind]
        baselines["core_prompt_detector"] = {
            "raw": core_prompt_kind,
            "label": mapped,
            "source": core_prompt_source
            or (
                "src-tauri/src/agent/state_reactor/tests.rs#live_codex_trust_frame_becomes_pending_prompt_through_state_reactor"
                if core_prompt_kind == "DirectoryTrust"
                else "src-tauri/src/service/output/manual_update_prompt_tests.rs#codex_update_dialog_is_a_respondable_structured_prompt"
            ),
        }
    rows.append(
        {
            "id": re.sub(r"[^a-z0-9]+", "-", source.lower()).strip("-"),
            "provider": provider,
            "boot_stage": boot_stage,
            "label": label,
            "source": source,
            "screen": screen,
            "baselines": baselines,
            "boot_stage_source": "src-tauri/src/service/output/manual_update_prompt_tests.rs#provider_ready_idle_user_dir_trust_prompt_appears_without_provenance_on_unchanged_screen"
            if boot_stage
            else None,
        }
    )


def add_inline(
    rows: list[dict[str, Any]],
    *,
    sample_id: str,
    provider: str,
    source: str,
    screen: str,
    label: str,
    baseline: tuple[str, dict[str, Any]] | None = None,
) -> None:
    if label not in LABELS:
        raise ValueError(f"unsupported label for {sample_id}: {label}")
    baselines = {baseline[0]: baseline[1]} if baseline else {}
    rows.append(
        {
            "id": sample_id,
            "provider": provider,
            "boot_stage": None,
            "label": label,
            "source": source,
            "screen": regex_scrub("\n".join(screen.splitlines()[-40:])),
            "baselines": baselines,
            "boot_stage_source": None,
        }
    )


def trust_expectations(provider: str) -> dict[str, str]:
    rel = (
        "plugins/provider-codex/tests/fixtures/directory-trust/cases.json"
        if provider == "codex"
        else "plugins/provider-claude/tests/fixtures/folder-trust/cases.json"
    )
    items = json.loads((ROOT / rel).read_text(encoding="utf-8"))
    return {item["file"]: item["expect"] for item in items}


def read_live_captures() -> list[dict[str, Any]]:
    if not LIVE_INPUT.exists():
        return []
    rows: list[dict[str, Any]] = []
    required = {"id", "provider", "boot_stage", "label", "source", "screen"}
    for line_number, line in enumerate(LIVE_INPUT.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip():
            continue
        try:
            source_row = json.loads(line)
        except json.JSONDecodeError as error:
            raise ValueError(f"invalid JSON in live capture {line_number}: {error.msg}") from error
        if not required <= source_row.keys():
            raise ValueError(f"live capture {line_number} is missing required fields")
        if source_row["label"] not in LABELS or not isinstance(source_row["screen"], str):
            raise ValueError(f"live capture {line_number} has an invalid label or screen")
        screen = regex_scrub(source_row["screen"].replace("\r\n", "\n").replace("\r", "\n"))
        rows.append(
            {
                "id": source_row["id"],
                "provider": source_row["provider"],
                "boot_stage": source_row["boot_stage"],
                "boot_stage_source": source_row.get("boot_stage_source"),
                "label": source_row["label"],
                "source": source_row["source"],
                "screen": "\n".join(screen.splitlines()[-40:]),
                "baselines": source_row.get("baselines", {}),
            }
        )
    return rows


def build() -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    codex_tab = [
        ("active-terminal-choice.txt", "trust_or_permission_prompt", "clarifying_question"),
        ("auth-login.txt", "login_or_auth_required", "waiting_on_auth"),
        ("diff-and-shell.txt", "crashed_to_shell", None),
        ("normal-working.txt", "working_normally", None),
        ("rate-limit-no-reset-time.txt", "model_unavailable_or_quota", "rate_limited"),
        ("rate-limit-try-again.txt", "model_unavailable_or_quota", "rate_limited"),
        ("stale-auth-ready-composer.txt", "ready_idle", None),
        ("stale-auth-streaming.txt", "working_normally", None),
        ("tool-call-list.txt", "working_normally", None),
        ("update-prompt-skip-option2.txt", "provider_update_prompt", "update_prompt"),
        ("update-prompt-skip-option3.txt", "provider_update_prompt", "update_prompt"),
    ]
    for name, label, tab_state in codex_tab:
        add_fixture(
            rows,
            "codex",
            f"plugins/provider-codex/tests/fixtures/tab-state/{name}",
            label,
            tab_state=tab_state,
        )

    claude_tab = [
        ("answered-clarifying-question.txt", "ready_idle", None),
        ("auth-login.txt", "login_or_auth_required", "waiting_on_auth"),
        ("normal-working.txt", "working_normally", None),
        ("prose-limit-mention.txt", "working_normally", None),
        ("rate-limit-menu.txt", "model_unavailable_or_quota", "rate_limited"),
        ("rate-limit-no-reset-time.txt", "model_unavailable_or_quota", "rate_limited"),
        ("rate-limit-wait-not-default.txt", "model_unavailable_or_quota", "rate_limited"),
        ("tool-call-list.txt", "working_normally", None),
    ]
    for name, label, tab_state in claude_tab:
        add_fixture(
            rows,
            "claude",
            f"plugins/provider-claude/tests/fixtures/tab-state/{name}",
            label,
            tab_state=tab_state,
        )

    codex_trust = [
        ("trust-48col-phrase-split.txt", "trust_or_permission_prompt"),
        ("bypass-warning.txt", "trust_or_permission_prompt"),
        ("trust-windows-focus-no.txt", "trust_or_permission_prompt"),
        ("trust-frame-full-screen-trailing-blank-rows.txt", "trust_or_permission_prompt"),
        ("approval-paragraph.txt", "trust_or_permission_prompt"),
        ("trust-structured-question.txt", "trust_or_permission_prompt"),
        ("command-approval-mentions-trust.txt", "trust_or_permission_prompt"),
        ("trust-windows-120col-wrapped.txt", "trust_or_permission_prompt"),
        ("trust-80col-git-note.txt", "trust_or_permission_prompt"),
        ("codex-update-available.txt", "provider_update_prompt"),
        ("trust-folder-access-0.157.1.txt", "trust_or_permission_prompt"),
    ]
    codex_expected = trust_expectations("codex")
    for name, label in codex_trust:
        source = f"plugins/provider-codex/tests/fixtures/directory-trust/{name}"
        add_fixture(
            rows,
            "codex",
            source,
            label,
            directory_trust_response=codex_expected.get(name),
            tab_state="update_prompt" if name == "codex-update-available.txt" else None,
            core_prompt_kind=(
                "ManualUpdate"
                if name == "codex-update-available.txt"
                else "DirectoryTrust"
                if name == "trust-frame-full-screen-trailing-blank-rows.txt"
                else None
            ),
            core_prompt_source=(
                "src-tauri/src/service/directory_trust/tests.rs#core_trust_decision_answers_claude_folder_trust_fixtures"
                if name in {
                    "trust-rule-focus-affirmative.txt",
                    "trust-box-focus-refusal.txt",
                    "trust-numbered-focus-affirmative.txt",
                }
                else None
            ),
            boot_stage="provider_ready" if name == "trust-folder-access-0.157.1.txt" else None,
        )

    claude_expected = trust_expectations("claude")
    rust_claude_trust = {
        "trust-rule-focus-affirmative.txt",
        "trust-box-focus-refusal.txt",
        "trust-numbered-focus-affirmative.txt",
    }
    for name, expected in sorted(claude_expected.items()):
        source = f"plugins/provider-claude/tests/fixtures/folder-trust/{name}"
        add_fixture(
            rows,
            "claude",
            source,
            "trust_or_permission_prompt",
            directory_trust_response=expected,
            core_prompt_kind="DirectoryTrust" if name in rust_claude_trust else None,
        )

    composer = [
        ("codex-0.157.1.txt", "ready_idle"),
        ("codex-0.154.0.txt", "ready_idle"),
        ("wrapped-warning.txt", "ready_idle"),
        ("composer-last-nonempty-row.txt", "ready_idle"),
    ]
    for name, label in composer:
        add_fixture(
            rows,
            "codex",
            f"plugins/provider-codex/tests/fixtures/composer-readiness/{name}",
            label,
        )

    run_watcher_source = "src-tauri/src/agent/run_watcher/tests.rs"
    for sample_id, screen in [
        ("watcher-working-propagating", "✢ Propagating… (3m 18s · ↑ 9.8k tokens)\n❯ "),
        ("watcher-working-schlepping", "✻ Schlepping… (12s · ↓ 2.1k tokens)"),
        ("watcher-working-interrupt", "working… esc to interrupt"),
    ]:
        add_inline(
            rows,
            sample_id=sample_id,
            provider="claude",
            source=f"{run_watcher_source}#screen_working_defers_idle_reap",
            screen=screen,
            label="working_normally",
            baseline=(
                "run_watcher_working",
                {
                    "raw": "screen_shows_working=true",
                    "label": "working_normally",
                    "source": run_watcher_source,
                },
            ),
        )

    for sample_id, provider, screen in [
        ("watcher-bare-shell-posix", "unknown", "agent@host:~/project$ "),
        ("watcher-bare-shell-powershell", "unknown", "PS C:\\Users\\agent> "),
    ]:
        add_inline(
            rows,
            sample_id=sample_id,
            provider=provider,
            source=f"{run_watcher_source}#bare_shell_detected_when_agent_gone",
            screen=screen,
            label="crashed_to_shell",
            baseline=(
                "run_watcher_bare_shell",
                {
                    "raw": "screen_is_bare_shell=true",
                    "label": "crashed_to_shell",
                    "source": run_watcher_source,
                },
            ),
        )

    add_fixture(
        rows,
        "claude",
        "plugins/provider-claude/tests/fixtures/wizard-multi-step.txt",
        "trust_or_permission_prompt",
    )
    add_fixture(
        rows,
        "opencode",
        "plugins/provider-opencode/tests/fixtures/sessionless-home-screen.txt",
        "ready_idle",
    )
    return rows


def main() -> None:
    rows = build()
    rows.extend(read_live_captures())
    ids = [row["id"] for row in rows]
    if len(rows) < 60 or len(set(ids)) != len(ids):
        raise SystemExit(f"corpus needs at least 60 unique records; got {len(rows)}")
    with OUT.open("w", encoding="utf-8", newline="\n") as output:
        for row in rows:
            output.write(json.dumps(row, ensure_ascii=False) + "\n")
    print(f"wrote {len(rows)} labeled screen records to {OUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
