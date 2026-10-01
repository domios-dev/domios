#!/usr/bin/env python3
"""Run the private question set against a dc-pinned dev instance."""

from __future__ import annotations

import argparse
import json
import math
import os
import statistics
import subprocess
import sys
import time
from collections import Counter, defaultdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from metrics import first_relevant_rank, gate_counts, reciprocal_rank


LOCAL_ROOT = Path.home() / "Developer" / "codeterm-worktrees" / "mem-eval-local"
QUESTIONS_PATH = LOCAL_ROOT / "questions.json"
MAP_PATH = LOCAL_ROOT / "id-map.json"
RESULTS_ROOT = LOCAL_ROOT / "results"
GATE_THRESHOLDS = (0.10, 0.15, 0.25, 0.35, 0.50)
POSITIVE_KINDS = {"answerable", "decoy_buried"}
ABSENT_KINDS = {"absent", "decoy_absent"}
DC_SHELL = 'source "$1"; shift; dc "$@"'
DOCS_CONFIG_KEY = "supporter.memory_rerank.sources.docs"
SESSIONS_CONFIG_KEY = "supporter.memory_rerank.sources.sessions"


def private_root() -> Path:
    repo_root = Path(__file__).resolve().parents[2]
    root = LOCAL_ROOT.resolve()
    if root.is_relative_to(repo_root):
        raise RuntimeError("questions and results must stay outside the repository")
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    os.chmod(root, 0o700)
    return root


def parent_environment() -> dict[str, str]:
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
        raise RuntimeError(f"cannot identify parent CodeTerm connection: {result.stderr.strip()}")
    identity = json.loads(result.stdout)
    connection = identity.get("connection", {})
    if connection.get("port") != 7685 or connection.get("instance"):
        raise RuntimeError(
            "run.py must start in the parent prod tab environment; "
            "the summary route is verified on port 7685"
        )
    if not env.get("CODETERM_PARENT_TAB_ID"):
        raise RuntimeError("CODETERM_PARENT_TAB_ID is required to route the summary")
    return env


def load_questions() -> list[dict[str, Any]]:
    raw = json.loads(QUESTIONS_PATH.read_text(encoding="utf-8"))
    questions = raw.get("questions") if isinstance(raw, dict) else raw
    if not isinstance(questions, list) or not questions:
        raise RuntimeError(f"question set is empty or malformed: {QUESTIONS_PATH}")
    seen: set[str] = set()
    for question in questions:
        required = {"id", "category", "query", "answer_ids", "kind", "note"}
        if not required <= question.keys():
            raise RuntimeError(f"question {question.get('id', '<unknown>')} is missing fields")
        session_answers = question.get("session_answer_ids", [])
        if not isinstance(session_answers, list) or not all(
            isinstance(session_id, str) for session_id in session_answers
        ):
            raise RuntimeError(f"question {question['id']} has invalid session_answer_ids")
        if question["id"] in seen:
            raise RuntimeError(f"duplicate question id: {question['id']}")
        if question["kind"] not in POSITIVE_KINDS | ABSENT_KINDS:
            raise RuntimeError(f"unsupported question kind: {question['kind']}")
        if question["kind"] in POSITIVE_KINDS and not (
            question["answer_ids"] or session_answers
        ):
            raise RuntimeError(f"positive question has no ground-truth ids: {question['id']}")
        if question["kind"] in ABSENT_KINDS and (question["answer_ids"] or session_answers):
            raise RuntimeError(f"absent question has ground-truth ids: {question['id']}")
        seen.add(question["id"])
    return questions


def run_dc_command(lib_path: Path, instance: str, args: list[str]) -> str:
    env = os.environ.copy()
    env["CODETERM_INSTANCE"] = instance
    result = subprocess.run(
        ["bash", "-c", DC_SHELL, "mem-rerank-live", str(lib_path), *args],
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode:
        raise RuntimeError(
            f"dc {' '.join(args[:2])} failed ({result.returncode}): "
            f"{result.stderr.strip() or result.stdout.strip()}"
        )
    return result.stdout


def run_dc(lib_path: Path, instance: str, args: list[str]) -> tuple[dict[str, Any], float]:
    started = time.perf_counter()
    output = run_dc_command(lib_path, instance, args)
    latency_ms = (time.perf_counter() - started) * 1000.0
    value = json.loads(output)
    if not isinstance(value, dict) or not isinstance(value.get("hits"), list):
        raise RuntimeError("mem search returned an unexpected JSON shape")
    return value, latency_ms


def read_docs_config(lib_path: Path, instance: str) -> int:
    output = run_dc_command(
        lib_path,
        instance,
        ["config", "get", DOCS_CONFIG_KEY, "--json"],
    )
    value = json.loads(output).get("value", 0)
    if type(value) is not int or not 0 <= value <= 10:
        raise RuntimeError(f"unexpected docs config value: {value!r}")
    return value


def read_sessions_config(lib_path: Path, instance: str) -> int:
    output = run_dc_command(
        lib_path,
        instance,
        ["config", "get", SESSIONS_CONFIG_KEY, "--json"],
    )
    value = json.loads(output).get("value", 0)
    if type(value) is not int or not 0 <= value <= 5:
        raise RuntimeError(f"unexpected sessions config value: {value!r}")
    return value


def write_docs_config(lib_path: Path, instance: str, docs: int) -> None:
    run_dc_command(
        lib_path,
        instance,
        ["config", "set", DOCS_CONFIG_KEY, str(docs), "--json"],
    )


def write_sessions_config(lib_path: Path, instance: str, sessions: int) -> None:
    run_dc_command(
        lib_path,
        instance,
        ["config", "set", SESSIONS_CONFIG_KEY, str(sessions), "--json"],
    )


def result_arm(value: dict[str, Any], latency_ms: float, answers: set[str]) -> dict[str, Any]:
    hits = []
    ids = []
    docs = []
    sessions = []
    for rank, hit in enumerate(value.get("hits", []), start=1):
        if hit.get("source") == "doc":
            pointer = hit.get("pointer")
            is_cli = isinstance(pointer, str) and pointer.endswith(" --help")
            doc = "CLI" if is_cli else pointer.removeprefix("codeterm docs ") if isinstance(pointer, str) else None
            ids.append("")
            docs.append(
                {
                    "rank": rank,
                    "doc": doc,
                    "section": pointer.removesuffix(" --help") if is_cli else None,
                    "pointer": pointer,
                    "via": hit.get("via"),
                    "score": hit.get("score"),
                    "relevance": hit.get("relevance"),
                }
            )
            continue
        if hit.get("source") == "session":
            session_id = hit.get("id")
            ids.append(session_id if isinstance(session_id, str) else "")
            sessions.append(
                {
                    "rank": rank,
                    "id": session_id,
                    "via": hit.get("via"),
                    "score": hit.get("score"),
                    "pointer": hit.get("pointer"),
                    "relevance": hit.get("relevance"),
                }
            )
            continue
        item_id = hit.get("id")
        if not isinstance(item_id, str):
            continue
        ids.append(item_id)
        hits.append(
            {
                "id": item_id,
                "score": hit.get("score"),
                "relevance": hit.get("relevance"),
                "is_ground_truth": item_id in answers,
            }
        )
    rank = first_relevant_rank(ids, answers)
    return {
        "first_ground_truth_rank": rank,
        "reciprocal_rank_at_10": reciprocal_rank(rank, 10),
        "hits": hits,
        "docs": docs,
        "sessions": sessions,
        "supporter": value.get("supporter"),
        "latency_ms": latency_ms,
    }


def nearest_rank(values: list[float], percentile: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    index = max(0, math.ceil(percentile * len(ordered)) - 1)
    return ordered[index]


def arm_metrics(rows: list[dict[str, Any]], arm: str) -> dict[str, Any]:
    positive_rows = [row for row in rows if row["kind"] in POSITIVE_KINDS]
    ranks = [row[arm]["first_ground_truth_rank"] for row in positive_rows]
    latencies = [row[arm]["latency_ms"] for row in rows]
    return {
        "positive_queries": len(positive_rows),
        "recall_at_1": sum(rank is not None and rank <= 1 for rank in ranks) / len(ranks)
        if ranks
        else None,
        "recall_at_3": sum(rank is not None and rank <= 3 for rank in ranks) / len(ranks)
        if ranks
        else None,
        "mrr_at_10": statistics.fmean(
            reciprocal_rank(rank, 10) for rank in ranks
        )
        if ranks
        else None,
        "latency_p50_ms": nearest_rank(latencies, 0.50),
        "latency_p95_ms": nearest_rank(latencies, 0.95),
        "latency_mean_ms": statistics.fmean(latencies) if latencies else None,
    }


def category_metrics(rows: list[dict[str, Any]]) -> dict[str, Any]:
    grouped: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        grouped[row["category"]].append(row)
    return {
        category: {
            "query_count": len(category_rows),
            "cosine": arm_metrics(category_rows, "cosine"),
            "rerank": arm_metrics(category_rows, "rerank"),
        }
        for category, category_rows in sorted(grouped.items())
    }


def comparison_counts(rows: list[dict[str, Any]]) -> dict[str, int]:
    counts = Counter()
    positive_rows = [row for row in rows if row["kind"] in POSITIVE_KINDS]
    for row in positive_rows:
        cosine = row["cosine"]["reciprocal_rank_at_10"]
        rerank = row["rerank"]["reciprocal_rank_at_10"]
        counts["better" if rerank > cosine else "worse" if rerank < cosine else "tie"] += 1
    return {
        "queries_compared": len(positive_rows),
        **{key: counts[key] for key in ("better", "worse", "tie")},
    }


def gate_metrics(rows: list[dict[str, Any]]) -> dict[str, Any]:
    absent_rows = [row for row in rows if row["kind"] in ABSENT_KINDS]
    relevant_rows = [row for row in rows if row["kind"] in POSITIVE_KINDS]
    scores = [
        envelope.get("max_relevance") if isinstance(envelope, dict) else None
        for envelope in [row["rerank"]["supporter"] for row in rows]
    ]
    absent_label = [row["kind"] in ABSENT_KINDS for row in rows]
    answerable_scores = [
        row["rerank"]["supporter"].get("max_relevance")
        if isinstance(row["rerank"]["supporter"], dict)
        else None
        for row in relevant_rows
    ]
    thresholds = {}
    for threshold in GATE_THRESHOLDS:
        metric = gate_counts(scores, absent_label, threshold)
        false_gates = sum(
            score is not None and score <= threshold for score in answerable_scores
        )
        thresholds[f"{threshold:.2f}"] = {
            **metric,
            "false_gates_on_answerable": false_gates,
        }
    return {
        "absent_queries": len(absent_rows),
        "answerable_queries": len(relevant_rows),
        "score_field": "rerank.supporter.max_relevance; <= threshold predicts absent",
        "thresholds": thresholds,
    }


def fallback_counts(rows: list[dict[str, Any]]) -> dict[str, int]:
    counts: Counter[str] = Counter()
    for row in rows:
        envelope = row["rerank"]["supporter"]
        if not isinstance(envelope, dict):
            continue
        status = envelope.get("status")
        if status in {"fallback", "skipped"}:
            counts[f"{status}:{envelope.get('reason', 'unknown')}" ] += 1
    return dict(sorted(counts.items()))


def docs_metrics(rows: list[dict[str, Any]]) -> dict[str, Any]:
    returned = [hit for row in rows for hit in row["rerank"]["docs"]]
    absent = [row for row in rows if row["kind"] in ABSENT_KINDS]
    by_doc = Counter(hit["doc"] for hit in returned if isinstance(hit.get("doc"), str))
    return {
        "queries_with_doc_hits": sum(bool(row["rerank"]["docs"]) for row in rows),
        "absent_queries_with_doc_hits": sum(bool(row["rerank"]["docs"]) for row in absent),
        "returned_doc_hits": len(returned),
        "by_doc": dict(sorted(by_doc.items())),
    }


def sessions_metrics(rows: list[dict[str, Any]]) -> dict[str, Any]:
    returned = [hit for row in rows for hit in row["rerank"]["sessions"]]
    absent = [row for row in rows if row["kind"] in ABSENT_KINDS]
    return {
        "queries_with_session_hits": sum(
            bool(row["rerank"]["sessions"]) for row in rows
        ),
        "absent_queries_with_session_hits": sum(
            bool(row["rerank"]["sessions"]) for row in absent
        ),
        "returned_session_hits": len(returned),
    }


def markdown_summary(
    timestamp: str,
    rows: list[dict[str, Any]],
    overall: dict[str, Any],
    per_category: dict[str, Any],
    gates: dict[str, Any],
    comparisons: dict[str, int],
    fallbacks: dict[str, int],
    docs: dict[str, Any],
    sessions: dict[str, Any],
) -> str:
    lines = [
        f"# Live memory decision re-rank evaluation: {timestamp}",
        "",
        f"Queries: {len(rows)}. Rank metrics use mapped memory IDs and optional provider session IDs.",
        "Latency is end-to-end CLI wall time through the dev-pinned `dc` wrapper.",
        "",
        "| Arm | Positive N | R@1 | R@3 | MRR@10 | p50 ms | p95 ms |",
        "|---|---:|---:|---:|---:|---:|---:|",
    ]
    for arm, label in (("cosine", "Cosine"), ("rerank", "Decision rerank")):
        values = overall[arm]
        lines.append(
            f"| {label} | {values['positive_queries']} | {fmt(values['recall_at_1'])} | "
            f"{fmt(values['recall_at_3'])} | {fmt(values['mrr_at_10'])} | "
            f"{fmt(values['latency_p50_ms'])} | {fmt(values['latency_p95_ms'])} |"
        )
    lines.extend(
        [
            "",
            f"Better / worse / tie on {comparisons['queries_compared']} positive queries by per-query MRR: {comparisons['better']} / "
            f"{comparisons['worse']} / {comparisons['tie']}.",
            "",
            "## Optional candidates",
            "",
            f"Queries returning docs: {docs['queries_with_doc_hits']}; absent probes returning docs: {docs['absent_queries_with_doc_hits']}; doc hits: {docs['returned_doc_hits']}.",
            f"Queries returning sessions: {sessions['queries_with_session_hits']}; absent probes returning sessions: {sessions['absent_queries_with_session_hits']}; session hits: {sessions['returned_session_hits']}.",
            "",
            "## Per category",
            "",
            "| Category | Positive N | Cosine R@1 / R@3 / MRR@10 | Rerank R@1 / R@3 / MRR@10 |",
            "|---|---:|---:|---:|",
        ]
    )
    for category, values in per_category.items():
        cosine = values["cosine"]
        rerank = values["rerank"]
        lines.append(
            f"| {category} | {cosine['positive_queries']} | "
            f"{fmt(cosine['recall_at_1'])} / {fmt(cosine['recall_at_3'])} / {fmt(cosine['mrr_at_10'])} | "
            f"{fmt(rerank['recall_at_1'])} / {fmt(rerank['recall_at_3'])} / {fmt(rerank['mrr_at_10'])} |"
        )
    lines.extend(
        [
            "",
            "## Absent-answer gate",
            "",
            "A gate prediction is `max_relevance <= threshold`; missing scores are not gated.",
            "",
            "| Threshold | Precision | Recall | TP | FP | FN | False gates on answerable |",
            "|---:|---:|---:|---:|---:|---:|---:|",
        ]
    )
    for threshold, values in gates["thresholds"].items():
        lines.append(
            f"| {threshold} | {fmt(values['precision'])} | {fmt(values['recall'])} | "
            f"{values['true_positive']} | {values['false_positive']} | {values['false_negative']} | "
            f"{values['false_gates_on_answerable']} |"
        )
    lines.extend(["", "## Fallbacks", ""])
    if fallbacks:
        lines.extend(f"- `{reason}`: {count}" for reason, count in fallbacks.items())
    else:
        lines.append("No fallback or skipped envelopes.")
    lines.append("")
    lines.append("Per-query details and raw envelopes are in the adjacent private `results.json`.")
    return "\n".join(lines) + "\n"


def fmt(value: float | int | None) -> str:
    return "—" if value is None else f"{value:.3f}" if isinstance(value, float) else str(value)


def evaluate_records(
    questions: list[dict[str, Any]],
    item_map: dict[str, str],
    lib_path: Path,
    instance: str,
) -> list[dict[str, Any]]:
    records = []
    for question in questions:
        mapped_answers = {item_map[item_id] for item_id in question["answer_ids"]}
        mapped_answers.update(question.get("session_answer_ids", []))
        arms = {}
        for arm_name, mode in (("cosine", "off"), ("rerank", "on")):
            value, latency_ms = run_dc(
                lib_path,
                instance,
                [
                    "mem",
                    "search",
                    question["query"],
                    "--json",
                    "--limit",
                    "10",
                    "--supporter",
                    mode,
                ],
            )
            arms[arm_name] = result_arm(value, latency_ms, mapped_answers)
        records.append(
            {
                **question,
                "mapped_answer_ids": sorted(mapped_answers),
                **arms,
            }
        )
    return records


def parse_source_count(value: str, label: str, maximum: int) -> int:
    try:
        count = int(value)
    except ValueError as error:
        raise argparse.ArgumentTypeError(
            f"{label} must be an integer from 0 to {maximum}"
        ) from error
    if not 0 <= count <= maximum:
        raise argparse.ArgumentTypeError(f"{label} must be an integer from 0 to {maximum}")
    return count


def run(args: argparse.Namespace) -> tuple[Path, Path, str]:
    root = private_root()
    parent_env = parent_environment()
    questions = load_questions()
    mapping_data = json.loads(MAP_PATH.read_text(encoding="utf-8"))
    item_map = mapping_data.get("items", {})
    missing = sorted({answer_id for q in questions for answer_id in q["answer_ids"] if answer_id not in item_map})
    if missing:
        raise RuntimeError(f"ground-truth ids missing from import map: {len(missing)}")

    lib_path = Path(__file__).resolve().parents[1] / "_lib" / "lib.sh"
    if args.docs is None and args.sessions is None:
        records = evaluate_records(questions, item_map, lib_path, args.dev_instance)
    else:
        previous_docs = (
            read_docs_config(lib_path, args.dev_instance) if args.docs is not None else None
        )
        previous_sessions = (
            read_sessions_config(lib_path, args.dev_instance)
            if args.sessions is not None
            else None
        )
        try:
            if args.docs is not None:
                write_docs_config(lib_path, args.dev_instance, args.docs)
            if args.sessions is not None:
                write_sessions_config(lib_path, args.dev_instance, args.sessions)
            records = evaluate_records(questions, item_map, lib_path, args.dev_instance)
        finally:
            try:
                if previous_docs is not None:
                    write_docs_config(lib_path, args.dev_instance, previous_docs)
            finally:
                if previous_sessions is not None:
                    write_sessions_config(lib_path, args.dev_instance, previous_sessions)

    overall = {
        "cosine": arm_metrics(records, "cosine"),
        "rerank": arm_metrics(records, "rerank"),
    }
    by_category = category_metrics(records)
    gates = gate_metrics(records)
    comparisons = comparison_counts(records)
    fallbacks = fallback_counts(records)
    docs = docs_metrics(records)
    sessions = sessions_metrics(records)
    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    result_dir = RESULTS_ROOT / timestamp
    result_dir.mkdir(parents=True, exist_ok=False, mode=0o700)
    os.chmod(result_dir, 0o700)
    result_path = result_dir / "results.json"
    summary_path = result_dir / "summary.md"
    summary = markdown_summary(
        timestamp,
        records,
        overall,
        by_category,
        gates,
        comparisons,
        fallbacks,
        docs,
        sessions,
    )
    payload = {
        "timestamp_utc": timestamp,
        "dev_instance": args.dev_instance,
        "decision_docs": args.docs,
        "decision_sessions": args.sessions,
        "docs": docs,
        "sessions": sessions,
        "question_count": len(records),
        "rank_metrics_include": sorted(POSITIVE_KINDS),
        "rank_source": "mapped memory IDs and optional provider session IDs, never titles",
        "overall": overall,
        "per_category": by_category,
        "better_worse_tie": comparisons,
        "gate": gates,
        "fallback_counts_by_reason": fallbacks,
        "queries": records,
    }
    result_path.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    summary_path.write_text(summary, encoding="utf-8")
    os.chmod(result_path, 0o600)
    os.chmod(summary_path, 0o600)
    parent_tab = parent_env["CODETERM_PARENT_TAB_ID"]
    delivered = subprocess.run(
        ["codeterm", "send", "--tab", parent_tab, "--from-file", str(summary_path)],
        env=parent_env,
        capture_output=True,
        text=True,
        check=False,
    )
    if delivered.returncode:
        raise RuntimeError(f"parent summary send failed: {delivered.stderr.strip()}")
    print(json.dumps({"results": str(result_path), "summary": str(summary_path), "send": delivered.stdout.strip()}))
    return result_path, summary_path, summary


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--dev-instance", default="memrerank")
    parser.add_argument("--docs", type=lambda value: parse_source_count(value, "docs", 10))
    parser.add_argument("--sessions", type=lambda value: parse_source_count(value, "sessions", 5))
    return parser.parse_args()


if __name__ == "__main__":
    try:
        run(parse_args())
    except Exception as error:
        print(f"run failed: {error}", file=sys.stderr)
        raise SystemExit(1)
