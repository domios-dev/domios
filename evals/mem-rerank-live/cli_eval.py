#!/usr/bin/env python3
"""Measure CLI mistake retrieval against the docs-enabled memrerank dev instance."""

from __future__ import annotations

import argparse
import json
import math
import os
import statistics
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LOCAL_ROOT = Path.home() / "Developer" / "codeterm-worktrees" / "mem-eval-local"
MISTAKES_PATH = Path(__file__).with_name("cli_mistakes.json")
RESULTS_ROOT = LOCAL_ROOT / "results" / "cli-mistakes"
DOCS_CONFIG_KEY = "memory.decision_rerank.sources.docs"
SESSIONS_CONFIG_KEY = "memory.decision_rerank.sources.sessions"
CONFIDENCE_THRESHOLDS = (0.50, 0.70, 0.85)
GATE_THRESHOLD = 0.70
GATE_HIT_AT_1 = 0.70
GATE_MAX_CONFIDENT_WRONG = 1


def private_root() -> Path:
    repo_root = Path(__file__).resolve().parents[2]
    root = LOCAL_ROOT.resolve()
    if root.is_relative_to(repo_root):
        raise RuntimeError("CLI eval results must stay outside the repository")
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    os.chmod(root, 0o700)
    return root


def parent_environment() -> tuple[dict[str, str], str]:
    env = os.environ.copy()
    env.pop("CODETERM_INSTANCE", None)
    result = subprocess.run(
        ["codeterm", "identify", "--json"],
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode:
        raise RuntimeError(
            f"cannot identify parent CodeTerm connection: {result.stderr.strip()}"
        )
    identity = json.loads(result.stdout)
    connection = identity.get("connection", {})
    if connection.get("port") != 7685 or connection.get("instance"):
        raise RuntimeError(
            "run cli_eval.py from the parent prod tab environment; "
            "the summary route must remain on port 7685"
        )
    parent_tab = env.get("CODETERM_PARENT_TAB_ID")
    if not parent_tab:
        raise RuntimeError("CODETERM_PARENT_TAB_ID is required to route the summary")
    return env, parent_tab


def cli_headings() -> set[str]:
    repo_root = Path(__file__).resolve().parents[2]
    reference = (repo_root / "generated" / "cli" / "cli.md").read_text(
        encoding="utf-8"
    )
    return {
        line.removeprefix("#### `").removesuffix("`")
        for line in reference.splitlines()
        if line.startswith("#### `codeterm ")
    }


def load_mistakes() -> list[dict[str, Any]]:
    value = json.loads(MISTAKES_PATH.read_text(encoding="utf-8"))
    if not isinstance(value, list) or not value:
        raise RuntimeError(f"mistake set is empty or malformed: {MISTAKES_PATH}")
    if len(value) != 15:
        raise RuntimeError(
            f"the gate is calibrated for exactly 15 mistakes, got {len(value)}"
        )
    headings = cli_headings()
    seen: set[str] = set()
    for index, mistake in enumerate(value, 1):
        required = {
            "command_as_typed",
            "error_text",
            "expected_cli_sections",
            "note",
        }
        if not isinstance(mistake, dict) or not required <= mistake.keys():
            raise RuntimeError(f"mistake {index} is missing required fields")
        command = mistake["command_as_typed"]
        error = mistake["error_text"]
        note = mistake["note"]
        sections = mistake["expected_cli_sections"]
        if not all(
            isinstance(text, str) and text.strip()
            for text in (command, error, note)
        ):
            raise RuntimeError(f"mistake {index} has an empty text field")
        if command in seen:
            raise RuntimeError(f"duplicate command in mistake set: {command}")
        if not isinstance(sections, list) or not sections or not all(
            isinstance(section, str) and section in headings for section in sections
        ):
            raise RuntimeError(f"mistake {index} names an unknown CLI section")
        seen.add(command)
    return value


def run_dc_command(lib_path: Path, instance: str, args: list[str]) -> str:
    env = os.environ.copy()
    env["CODETERM_INSTANCE"] = instance
    result = subprocess.run(
        [
            "bash",
            "-c",
            'source "$1"; shift; dc "$@"',
            "cli-eval",
            str(lib_path),
            *args,
        ],
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode:
        raise RuntimeError(
            f"dc {' '.join(args[:3])} failed ({result.returncode}): "
            f"{result.stderr.strip() or result.stdout.strip()}"
        )
    return result.stdout


def config_integer(lib_path: Path, instance: str, key: str, maximum: int) -> int:
    output = run_dc_command(lib_path, instance, ["config", "get", key, "--json"])
    value = json.loads(output).get("value")
    if type(value) is not int or not 0 <= value <= maximum:
        raise RuntimeError(f"unexpected {key} config value: {value!r}")
    return value


def number(value: Any) -> float | None:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        return None
    numeric = float(value)
    return numeric if math.isfinite(numeric) and 0 <= numeric <= 1 else None


def is_expected_cli_hit(hit: Any, expected_sections: set[str]) -> bool:
    return (
        isinstance(hit, dict)
        and hit.get("source") == "doc"
        and hit.get("doc") == "CLI"
        and hit.get("section") in expected_sections
    )


def evaluate_one(
    mistake: dict[str, Any], lib_path: Path, instance: str
) -> dict[str, Any]:
    query = f"{mistake['command_as_typed']} {mistake['error_text']}"
    started = time.perf_counter()
    output = run_dc_command(
        lib_path,
        instance,
        ["mem", "search", query, "--decision-rerank", "on", "--json"],
    )
    latency_ms = (time.perf_counter() - started) * 1000.0
    value = json.loads(output)
    hits = value.get("hits")
    if not isinstance(hits, list):
        raise RuntimeError("mem search returned an unexpected JSON shape")
    expected = set(mistake["expected_cli_sections"])
    correct_rank = next(
        (
            rank
            for rank, hit in enumerate(hits, 1)
            if is_expected_cli_hit(hit, expected)
        ),
        None,
    )
    top = hits[0] if hits else None
    top_relevance = number(top.get("relevance")) if isinstance(top, dict) else None
    top_is_cli = (
        isinstance(top, dict)
        and top.get("source") == "doc"
        and top.get("doc") == "CLI"
    )
    return {
        "note": mistake["note"],
        "command_as_typed": mistake["command_as_typed"],
        "expected_cli_sections": mistake["expected_cli_sections"],
        "correct_rank": correct_rank,
        "top1_relevance": top_relevance,
        "top1_is_cli_doc": top_is_cli,
        "top1_is_correct": correct_rank == 1,
        "top1_doc": top.get("doc") if isinstance(top, dict) else None,
        "top1_section": top.get("section") if isinstance(top, dict) else None,
        "decision_rerank": value.get("decision_rerank"),
        "latency_ms": latency_ms,
    }


def confident_wrong_counts(rows: list[dict[str, Any]]) -> dict[str, int]:
    return {
        f"{threshold:.2f}": sum(
            row["top1_relevance"] is not None
            and row["top1_relevance"] >= threshold
            and not row["top1_is_correct"]
            for row in rows
        )
        for threshold in CONFIDENCE_THRESHOLDS
    }


def median_relevance(rows: list[dict[str, Any]], correct: bool) -> float | None:
    values = [
        row["top1_relevance"]
        for row in rows
        if row["top1_is_correct"] is correct and row["top1_relevance"] is not None
    ]
    return statistics.median(values) if values else None


def metrics(rows: list[dict[str, Any]]) -> dict[str, Any]:
    total = len(rows)
    hit_at_1 = sum(row["correct_rank"] == 1 for row in rows) / total
    hit_at_3 = sum(
        row["correct_rank"] is not None and row["correct_rank"] <= 3
        for row in rows
    ) / total
    confident_wrong = confident_wrong_counts(rows)
    return {
        "cases": total,
        "hit_at_1": hit_at_1,
        "hit_at_3": hit_at_3,
        "median_correct_top1_relevance": median_relevance(rows, True),
        "median_wrong_top1_relevance": median_relevance(rows, False),
        "confident_but_wrong": confident_wrong,
        "top1_cli_doc_hits": sum(row["top1_is_cli_doc"] for row in rows),
        "applied_queries": sum(
            isinstance(row["decision_rerank"], dict)
            and row["decision_rerank"].get("status") == "applied"
            for row in rows
        ),
        "latency_p50_ms": statistics.median(row["latency_ms"] for row in rows),
    }


def gate_outcome(values: dict[str, Any]) -> str:
    confident_wrong = values["confident_but_wrong"][f"{GATE_THRESHOLD:.2f}"]
    if (
        values["hit_at_1"] >= GATE_HIT_AT_1
        and confident_wrong <= GATE_MAX_CONFIDENT_WRONG
    ):
        return "PROCEED with the CLI error hint task"
    return "CANCEL the CLI error hint task"


def fmt_percent(value: float | None) -> str:
    return "n/a" if value is None else f"{value * 100:.1f}%"


def fmt_relevance(value: float | None) -> str:
    return "n/a" if value is None else f"{value:.3f}"


def markdown_summary(
    instance: str,
    docs_count: int,
    sessions_count: int,
    values: dict[str, Any],
    rows: list[dict[str, Any]],
) -> str:
    confident = values["confident_but_wrong"]
    lines = [
        "# CLI mistake decision re-rank evaluation",
        "",
        f"Instance: `{instance}`. Cases: {values['cases']}. Docs candidates: {docs_count}. Sessions candidates: {sessions_count}.",
        "Relevance compares the top result with the expected `doc=CLI` section title.",
        "",
        f"- Hit@1: {fmt_percent(values['hit_at_1'])}",
        f"- Hit@3: {fmt_percent(values['hit_at_3'])}",
        f"- Median top-1 relevance when correct: {fmt_relevance(values['median_correct_top1_relevance'])}",
        f"- Median top-1 relevance when wrong: {fmt_relevance(values['median_wrong_top1_relevance'])}",
        f"- Confident but wrong at 0.50 / 0.70 / 0.85: {confident['0.50']} / {confident['0.70']} / {confident['0.85']}",
        f"- Top-1 CLI doc hits: {values['top1_cli_doc_hits']} / {values['cases']}",
        f"- Applied searches: {values['applied_queries']} / {values['cases']}",
        f"- p50 CLI latency: {values['latency_p50_ms']:.1f} ms",
        "",
        f"Gate threshold: confident-but-wrong at {GATE_THRESHOLD:.2f} must be <= {GATE_MAX_CONFIDENT_WRONG}/{values['cases']}; hit@1 must be >= {fmt_percent(GATE_HIT_AT_1)}.",
        f"Gate outcome: **{gate_outcome(values)}**.",
        "",
        "| # | Mistake | Correct rank | Top-1 relevance | Top-1 CLI doc | Top-1 section |",
        "|---:|---|---:|---:|:---:|---|",
    ]
    for index, row in enumerate(rows, 1):
        rank = "miss" if row["correct_rank"] is None else str(row["correct_rank"])
        section = row["top1_section"] or ""
        note = row["note"].replace("|", "\\|")
        section = section.replace("|", "\\|")
        lines.append(
            f"| {index} | {note} | {rank} | {fmt_relevance(row['top1_relevance'])} | "
            f"{'yes' if row['top1_is_cli_doc'] else 'no'} | {section} |"
        )
    lines.append("")
    lines.append("Per-case JSON is in the adjacent local results directory.")
    return "\n".join(lines) + "\n"


def save_and_send(
    payload: dict[str, Any],
    summary: str,
    parent_env: dict[str, str],
    parent_tab: str,
) -> tuple[Path, Path]:
    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    result_dir = RESULTS_ROOT / timestamp
    result_dir.mkdir(parents=True, exist_ok=False, mode=0o700)
    os.chmod(result_dir, 0o700)
    result_path = result_dir / "results.json"
    summary_path = result_dir / "summary.md"
    result_path.write_text(
        json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    summary_path.write_text(summary, encoding="utf-8")
    os.chmod(result_path, 0o600)
    os.chmod(summary_path, 0o600)
    delivered = subprocess.run(
        [
            "codeterm",
            "send",
            "--tab",
            parent_tab,
            "--require-agent",
            "--from-file",
            str(summary_path),
        ],
        env=parent_env,
        capture_output=True,
        text=True,
        check=False,
    )
    if delivered.returncode:
        raise RuntimeError(f"parent summary send failed: {delivered.stderr.strip()}")
    print(
        json.dumps(
            {
                "results": str(result_path),
                "summary": str(summary_path),
                "send": delivered.stdout.strip(),
            }
        )
    )
    return result_path, summary_path


def run(args: argparse.Namespace) -> tuple[Path, Path, str]:
    private_root()
    parent_env, parent_tab = parent_environment()
    mistakes = load_mistakes()
    lib_path = Path(__file__).resolve().parents[1] / "_lib" / "lib.sh"
    docs_count = config_integer(lib_path, args.dev_instance, DOCS_CONFIG_KEY, 10)
    if docs_count < 3:
        raise RuntimeError(
            f"docs source is {docs_count}; the CLI corpus evaluation requires docs >= 3"
        )
    sessions_count = config_integer(lib_path, args.dev_instance, SESSIONS_CONFIG_KEY, 5)
    if sessions_count != 0:
        raise RuntimeError(
            f"sessions source is {sessions_count}; this measurement expects the shipping default 0"
        )
    rows = [evaluate_one(mistake, lib_path, args.dev_instance) for mistake in mistakes]
    values = metrics(rows)
    summary = markdown_summary(
        args.dev_instance,
        docs_count,
        sessions_count,
        values,
        rows,
    )
    payload = {
        "timestamp_utc": datetime.now(timezone.utc).isoformat(),
        "dev_instance": args.dev_instance,
        "docs_candidates": docs_count,
        "sessions_candidates": sessions_count,
        "confidence_thresholds": list(CONFIDENCE_THRESHOLDS),
        "gate_threshold": GATE_THRESHOLD,
        "metrics": values,
        "gate_outcome": gate_outcome(values),
        "queries": rows,
    }
    results_path, summary_path = save_and_send(
        payload,
        summary,
        parent_env,
        parent_tab,
    )
    return results_path, summary_path, summary


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--dev-instance", default="memrerank")
    return parser.parse_args()


if __name__ == "__main__":
    try:
        run(parse_args())
    except Exception as error:
        print(f"CLI eval failed: {error}", file=sys.stderr)
        raise SystemExit(1)
