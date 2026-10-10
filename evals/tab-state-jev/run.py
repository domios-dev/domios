#!/usr/bin/env python3
"""Measure screen-only and context-aware tab-state classification."""

from __future__ import annotations

import argparse
import json
import math
import os
import statistics
import sys
import time
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any
import heldout
import request_contract as contract
from jev_transport import JevTransport, TransportFailure, token_bytes


ROOT = Path(__file__).resolve().parents[2]
DATASET = Path(__file__).with_name("screens.jsonl")
CRITERIA_FILE = Path(__file__).with_name("criteria.json")
DEFAULT_ENDPOINT = "https://api.typesafe.ai/v1/systemone"
DEFAULT_MODEL = "jev-latest"
MAX_CALLS_PER_RUN = 300
STRONG_THRESHOLD = 0.80
WEAK_THRESHOLD = 0.60
STRONG_GATE = 0.90
WEAK_GATE = 0.60
MIN_FRAMES_PER_LABEL = 4
REQUIRED_TRANSPORTS = ("pty", "chat_backend")
LABELS = (
    "ready_idle",
    "working_normally",
    "provider_update_prompt",
    "trust_or_permission_prompt",
    "login_or_auth_required",
    "model_unavailable_or_quota",
    "crashed_to_shell",
    "error_loop",
    "still_starting",
    "other",
)
ACTIONABLE = {
    "provider_update_prompt",
    "trust_or_permission_prompt",
    "login_or_auth_required",
    "model_unavailable_or_quota",
    "crashed_to_shell",
    "error_loop",
}
CONTEXT_FIELDS = (
    "provider_capability",
    "requested_model",
    "session_model",
    "role",
    "plan_id",
    "task_id",
    "spawned_by_tab",
    "seconds_since_spawn",
    "seconds_since_last_output",
    "structural_verdict",
    "pending_prompt_kind",
    "screen_changed",
)
INSTRUCTIONS = "Classify the current agent tab state from its visible terminal screen. Choose the single best label."
CONTEXT_INSTRUCTIONS = "Classify the current agent tab state using the screen and runtime context. Choose the single best label."


def band_for_confidence(confidence: float) -> str:
    if isinstance(confidence, bool) or not isinstance(confidence, (int, float)):
        raise ValueError("confidence must be numeric")
    if not math.isfinite(confidence) or not 0 <= confidence <= 1:
        raise ValueError("confidence must be finite and between zero and one")
    if confidence >= STRONG_THRESHOLD:
        return "strong"
    if confidence >= WEAK_THRESHOLD:
        return "weak"
    return "ignored"


def coverage_gaps(samples: list[dict[str, Any]]) -> list[str]:
    support = Counter(sample["label"] for sample in samples)
    gaps = [
        f"{label}={support[label]}"
        for label in LABELS
        if support[label] < MIN_FRAMES_PER_LABEL
    ]
    transports = {sample["transport"] for sample in samples}
    gaps.extend(f"transport/{transport}=0" for transport in REQUIRED_TRANSPORTS if transport not in transports)
    return gaps


def read_dataset(path: Path, *, require_coverage: bool = True, raw: bytes | None = None) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    required = {
        "id",
        "provider",
        "transport",
        "provider_capability",
        "label",
        "labelled_by",
        "source",
        "screen",
        "context",
    }
    raw = contract.bounded_file(path, contract.MAX_DATASET) if raw is None else raw
    contract.require(type(raw) is bytes and len(raw) <= contract.MAX_DATASET, "dataset_size")
    for line_number, line in enumerate(raw.decode("utf-8").splitlines(), 1):
        if not line.strip():
            continue
        try:
            row = contract.decode(line.encode("utf-8"), contract.MAX_REQUEST)
        except json.JSONDecodeError as error:
            raise ValueError(f"invalid JSON on {path}:{line_number}: {error.msg}") from error
        if not isinstance(row, dict) or not required <= row.keys():
            raise ValueError(f"{path}:{line_number} is missing required fields")
        contract.require(type(row["id"]) is str and 0 < len(row["id"]) <= 256, "dataset_identity")
        if row["label"] not in LABELS or not isinstance(row["screen"], str) or not row["screen"].strip():
            raise ValueError(f"{path}:{line_number} has an invalid label or empty screen")
        if (
            not isinstance(row["provider"], str)
            or not row["provider"]
            or not isinstance(row["labelled_by"], str)
            or not row["labelled_by"].strip()
            or not isinstance(row["source"], str)
            or not row["source"].startswith("dev-capture")
        ):
            raise ValueError(f"{path}:{line_number} is not a human-labelled dev capture")
        capability = row["provider_capability"]
        if (
            not isinstance(capability, dict)
            or not isinstance(capability.get("transport"), str)
            or capability.get("transport") not in {"pty", "chat_backend"}
            or not isinstance(capability.get("display_name"), str)
            or not capability["display_name"].strip()
            or row["transport"] != capability["transport"]
        ):
            raise ValueError(f"{path}:{line_number} has no provider capability transport")
        context = row["context"]
        if not isinstance(context, dict) or not set(CONTEXT_FIELDS) <= context.keys():
            raise ValueError(f"{path}:{line_number} is missing runtime context fields")
        if context["provider_capability"] != capability:
            raise ValueError(f"{path}:{line_number} context capability does not match the tab descriptor")
        if not isinstance(context["screen_changed"], bool):
            raise ValueError(f"{path}:{line_number} has an invalid screen_changed value")
        for field in ("seconds_since_spawn", "seconds_since_last_output"):
            value = context[field]
            if (
                isinstance(value, bool)
                or not isinstance(value, (int, float))
                or not math.isfinite(value)
                or value < 0
            ):
                raise ValueError(f"{path}:{line_number} has an invalid {field}")
        rows.append(row)
    ids = [row["id"] for row in rows]
    if len(set(ids)) != len(ids):
        raise ValueError("the dataset contains duplicate ids")
    if not rows:
        raise ValueError("screens.jsonl has no labelled real dev captures")
    gaps = coverage_gaps(rows)
    if gaps and require_coverage:
        raise ValueError(
            f"the labelled set needs at least {MIN_FRAMES_PER_LABEL} real frames per label "
            "and both pty/chat_backend transports; "
            f"missing: {', '.join(gaps)}"
        )
    return rows


def load_criteria(path: Path, variant: str = "current", *, mode: str = "canonical") -> dict[str, str]:
    contract.require(mode in ("canonical", "historical-diagnostic"), "criteria_mode")
    contract.require(mode != "canonical" or variant == "current", "criteria_mode_conflict")
    return contract.load_criteria_bytes(contract.bounded_file(path, contract.MAX_CRITERIA),
                                        variant=variant if mode == "historical-diagnostic" else None)


def load_token(token_file: Path | None) -> str:
    for name in ("JEV_API_TOKEN", "TYPESAFE_JEV_TOKEN"):
        if name in os.environ:
            token = os.environ[name]
            token_bytes(token)
            return token
    if token_file is None:
        raise TransportFailure("token_preflight")
    try:
        value = contract.decode(contract.bounded_file(token_file, 16 * 1024), 16 * 1024)
    except (OSError, contract.ContractError) as error:
        raise TransportFailure("token_preflight") from error
    token = value.get("token") if isinstance(value, dict) else None
    token_bytes(token)
    return token


def default_questions(arm: str, criteria: dict[str, str]) -> dict[str, Any]:
    contract.require(arm in ("screen", "context"), "arm")
    contract.criteria_map(criteria)
    name = "q" if arm == "screen" else "label"
    questions = {name: {
        "type": "choice",
        "instructions": INSTRUCTIONS if arm == "screen" else CONTEXT_INSTRUCTIONS,
        "criteria": criteria,
    }}
    if arm == "context":
        questions["prompt_visible"] = {
            "type": "noul",
            "instructions": "Is an interactive prompt awaiting a response on the current screen?",
            "criteria": {
                "true": "A currently visible interactive prompt is waiting for input or a choice.",
                "false": "There is no currently visible interactive prompt waiting for input; historical transcript prompts do not count.",
            },
        }
    return questions


def request_body(
    screen: str,
    context: dict[str, Any],
    *,
    arm: str,
    model: str,
    criteria: dict[str, str],
    question_manifest: bytes | None = None,
) -> bytes:
    expected = default_questions(arm, criteria)
    questions = expected if question_manifest is None else contract.decode(question_manifest, contract.MAX_REQUEST)
    contract.validate_questions(questions)
    label_name = "q" if arm == "screen" else "label"
    contract.require(set(expected) <= set(questions), "missing_classification_questions")
    contract.require(questions[label_name]["type"] == "choice" and
                     questions[label_name]["criteria"] == criteria, "classification_criteria_mismatch")
    if arm == "context":
        contract.require(questions["prompt_visible"]["type"] == "noul", "prompt_visibility_question")
    contract.require(type(screen) is str and type(context) is dict, "state_schema")
    state: Any = screen if arm == "screen" else {"screen": screen, "context": context}
    raw = contract.encode({"state": state, "model": model, "questions": questions})
    contract.validate_wire(raw)
    return raw


def call_jev(
    screen: str,
    context: dict[str, Any],
    *,
    arm: str,
    criteria: dict[str, str],
    token: str,
    endpoint: str,
    model: str,
    timeout: float,
    question_manifest: bytes | None = None,
    transport: Any = None,
) -> tuple[dict[str, Any] | None, float, str | None]:
    started = time.monotonic()
    try:
        raw = request_body(screen, context, arm=arm, model=model, criteria=criteria,
                           question_manifest=question_manifest)
        transport = JevTransport() if transport is None else transport
        result = transport.send(raw, token=token, timeout=timeout, endpoint=endpoint)
        questions = contract.validate_wire(raw)["questions"]
        answers = contract.validate_answers(result.payload, questions)
        label_name = "q" if arm == "screen" else "label"
        answer = answers[label_name]
        result_source = result.payload["answers"][label_name].get("confidence_source", answer["confidence_source"])
        return {
            "prediction": answer["choice"], "confidence": answer["confidence"],
            "confidence_source": result_source,
            "prompt_visible": answers["prompt_visible"]["noul"] if arm == "context" else None,
            "probabilities": answer["probabilities"], "declared_answers": answers,
            **contract.wire_hashes(raw),
        }, (time.monotonic() - started) * 1000, None
    except (TransportFailure, contract.ContractError) as error:
        return None, (time.monotonic() - started) * 1000, error.code


def precision_recall(
    samples: list[dict[str, Any]], predictions: list[dict[str, Any]]
) -> dict[str, dict[str, Any]]:
    by_label: dict[str, dict[str, Any]] = {}
    for label in LABELS:
        actual = [sample for sample in samples if sample["label"] == label]
        predicted = [item for item in predictions if item["prediction"] == label]
        correct = sum(item["label"] == label for item in predicted)
        by_label[label] = {
            "support": len(actual),
            "predicted": len(predicted),
            "correct": correct,
            "precision": correct / len(predicted) if predicted else None,
            "recall": correct / len(actual) if actual else None,
        }
    return by_label


def band_metrics(
    samples: list[dict[str, Any]], predictions: list[dict[str, Any]]
) -> dict[str, Any]:
    by_band: dict[str, Any] = {}
    for band in ("strong", "weak", "ignored"):
        selected = [item for item in predictions if band_for_confidence(item["confidence"]) == band]
        correct = sum(item["prediction"] == item["label"] for item in selected)
        per_label = {}
        for label in LABELS:
            predicted_label = [item for item in selected if item["prediction"] == label]
            label_correct = sum(item["label"] == label for item in predicted_label)
            support = sum(sample["label"] == label for sample in samples)
            per_label[label] = {
                "predicted": len(predicted_label),
                "correct": label_correct,
                "precision": label_correct / len(predicted_label) if predicted_label else None,
                "recall": label_correct / support if support else None,
            }
        by_band[band] = {
            "predicted": len(selected),
            "correct": correct,
            "precision": correct / len(selected) if selected else None,
            "per_label": per_label,
        }
    strong_actionable = [
        item
        for item in predictions
        if band_for_confidence(item["confidence"]) == "strong" and item["prediction"] in ACTIONABLE
    ]
    strong_correct = sum(item["prediction"] == item["label"] for item in strong_actionable)
    by_band["strong_actionable"] = {
        "predicted": len(strong_actionable),
        "correct": strong_correct,
        "precision": strong_correct / len(strong_actionable) if strong_actionable else None,
        "per_label": {
            label: by_band["strong"]["per_label"][label]
            for label in sorted(ACTIONABLE)
        },
    }
    return by_band


def aggregate_precision(
    samples: list[dict[str, Any]],
    predictions: list[dict[str, Any]],
    field: str,
) -> dict[str, dict[str, Any]]:
    predicted_by_id = {item["id"]: item for item in predictions}
    sample_groups: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for sample in samples:
        sample_groups[str(sample[field])].append(sample)
    report: dict[str, dict[str, Any]] = {}
    for group, group_samples in sorted(sample_groups.items()):
        group_predictions = [
            predicted_by_id[sample["id"]]
            for sample in group_samples
            if sample["id"] in predicted_by_id
        ]
        correct = sum(item["prediction"] == item["label"] for item in group_predictions)
        report[group] = {
            "support": len(group_samples),
            "calls": len(group_predictions),
            "correct": correct,
            "precision": correct / len(group_predictions) if group_predictions else None,
        }
    return report


def aggregate_band_precision(
    samples: list[dict[str, Any]],
    predictions: list[dict[str, Any]],
    field: str,
) -> dict[str, dict[str, dict[str, Any]]]:
    return {
        band: aggregate_precision(
            samples,
            [item for item in predictions if band_for_confidence(item["confidence"]) == band],
            field,
        )
        for band in ("strong", "weak", "ignored")
    }


def gate_verdict(
    strong_actionable_precision: float | None,
    weak_precision: float | None,
) -> dict[str, Any]:
    strong_pass = strong_actionable_precision is not None and strong_actionable_precision >= STRONG_GATE
    weak_pass = weak_precision is not None and weak_precision >= WEAK_GATE
    return {
        "strong_actionable": {
            "precision": strong_actionable_precision,
            "threshold": STRONG_GATE,
            "verdict": "pass" if strong_pass else "fail",
        },
        "weak_overall": {
            "precision": weak_precision,
            "threshold": WEAK_GATE,
            "verdict": "pass" if weak_pass else "fail",
        },
        "verdict": "pass" if strong_pass and weak_pass else "fail",
    }


def nearest_rank(values: list[float], percentile: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    index = max(0, math.ceil(percentile * len(ordered)) - 1)
    return ordered[index]


def evaluate(
    samples: list[dict[str, Any]],
    *,
    arm: str,
    criteria: dict[str, str],
    token: str,
    endpoint: str,
    model: str,
    timeout: float,
    manifest: contract.Manifest | None = None,
    complete_dataset: bool = True,
    dataset_set: str = "tuning",
    transport: Any = None,
) -> dict[str, Any]:
    coverage_missing = coverage_gaps(samples)
    contract.require(dataset_set in ("tuning", "heldout"), "dataset_set")
    admission_gaps = coverage_missing if dataset_set == "tuning" else heldout.coverage_gaps(samples)
    if admission_gaps or not complete_dataset:
        raise ValueError("incomplete_dataset: " + ", ".join(admission_gaps))
    contract.require(0 < len(samples) <= MAX_CALLS_PER_RUN, "request_count")
    contract.require(manifest is not None and manifest.endpoint == endpoint and manifest.model_alias == model,
                     "immutable_manifest_required")
    contract.require(samples == read_dataset(DATASET, raw=manifest.dataset_bytes,
                     require_coverage=dataset_set == "tuning"), "manifest_dataset_mismatch")
    for sample in samples:
        request_body(sample["screen"], sample["context"], arm=arm, model=model, criteria=criteria,
                     question_manifest=manifest.question_bytes)
    transport = JevTransport() if transport is None else transport
    predictions: list[dict[str, Any]] = []
    latencies: list[float] = []
    failures: Counter[str] = Counter()
    for sample in samples:
        answer, latency_ms, error = call_jev(
            sample["screen"],
            sample["context"],
            arm=arm,
            criteria=criteria,
            token=token,
            endpoint=endpoint,
            model=model,
            timeout=timeout,
            question_manifest=manifest.question_bytes,
            transport=transport,
        )
        latencies.append(latency_ms)
        record: dict[str, Any] = {
            "id": sample["id"],
            "label": sample["label"],
            "provider": sample["provider"],
            "transport": sample["transport"],
            "latency_ms": round(latency_ms, 2),
        }
        if error:
            record["error"] = error
            failures[error] += 1
        else:
            record.update(answer or {})
        predictions.append(record)
        if transport.last_receipt is not None:
            record["transport_receipt"] = transport.last_receipt
        if error:
            break

    successful = [item for item in predictions if "prediction" in item]
    bands = band_metrics(samples, successful)
    observed_gate = gate_verdict(
        bands["strong_actionable"]["precision"],
        bands["weak"]["precision"],
    )
    gate = dict(observed_gate)
    gate["eligible"] = not coverage_missing and len(successful) == len(samples) and complete_dataset
    if not gate["eligible"]:
        gate["verdict"] = "inconclusive"
    return {
        "arm": arm,
        "question_names": list(manifest.questions),
        "manifest": manifest.report(samples),
        **contract.qualification_status(samples),
        "model": model,
        "endpoint": endpoint,
        "dataset": str(DATASET.relative_to(ROOT)),
        "snapshot_count": len(samples),
        "calls_used": len(predictions),
        "successful_calls": len(successful),
        "failed_calls": dict(sorted(failures.items())),
        "per_label": precision_recall(samples, successful),
        "bands": bands,
        "per_provider": aggregate_precision(samples, successful, "provider"),
        "per_transport": aggregate_precision(samples, successful, "transport"),
        "per_band_per_provider": aggregate_band_precision(samples, successful, "provider"),
        "per_band_per_transport": aggregate_band_precision(samples, successful, "transport"),
        "coverage": {
            "complete": not coverage_missing,
            "missing_groups": coverage_missing,
        },
        "observed_gate": observed_gate,
        "gate": gate,
        "latency_ms": {
            "mean": statistics.fmean(latencies) if latencies else None,
            "p50": nearest_rank(latencies, 0.50),
            "p95": nearest_rank(latencies, 0.95),
            "n": len(latencies),
        },
        "predictions": predictions,
    }


def parse_args(argv=None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--set", choices=("tuning", "heldout"), default="tuning")
    parser.add_argument("--dataset", type=Path)
    parser.add_argument("--criteria-file", type=Path)
    parser.add_argument("--criteria-mode", choices=("canonical", "historical-diagnostic"), default="canonical")
    parser.add_argument("--criteria-variant", choices=("current", "auth-first-run"), default="current")
    parser.add_argument("--question-manifest", type=Path)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--arm", choices=("screen", "context"), default="screen")
    parser.add_argument("--token-file", type=Path)
    parser.add_argument("--endpoint", choices=(DEFAULT_ENDPOINT,), default=DEFAULT_ENDPOINT)
    parser.add_argument("--model", default=DEFAULT_MODEL)
    parser.add_argument("--offset", type=int, default=0)
    parser.add_argument("--limit", type=int)
    parser.add_argument("--timeout", type=float, default=60.0)
    return parser.parse_args(argv)


def main() -> int:
    args = parse_args()
    try:
        contract.validate_source_sha(args.source_sha)
        contract.require(math.isfinite(args.timeout) and 5 < args.timeout <= 60, "request_budget")
        contract.require(args.offset == 0, "partial_dataset_forbidden")
        contract.require(args.criteria_mode != "canonical" or args.criteria_variant == "current", "criteria_mode_conflict")
        if args.dataset is None:
            args.dataset = DATASET if args.set == "tuning" else DATASET.with_name("heldout.jsonl")
        if args.criteria_file is None:
            args.criteria_file = CRITERIA_FILE if args.criteria_mode == "canonical" else CRITERIA_FILE.with_name("criteria_variants.json")
        criteria_raw = contract.bounded_file(args.criteria_file, contract.MAX_CRITERIA)
        criteria = contract.load_criteria_bytes(criteria_raw,
                    variant=args.criteria_variant if args.criteria_mode == "historical-diagnostic" else None)
        if args.criteria_mode == "historical-diagnostic":
            print(json.dumps({"criteria_mode": args.criteria_mode, "criteria_variant": args.criteria_variant,
                "criteria_bytes_sha256": contract.sha256(criteria_raw), "source_sha": args.source_sha,
                "source_sha_origin": "operator_declared_unverified", "missing_labels": sorted(set(LABELS) - set(criteria)),
                "inference_permitted": False, **contract.qualification_status([])}, indent=2))
            return 0
        dataset_raw = contract.bounded_file(args.dataset, contract.MAX_DATASET)
        samples = read_dataset(args.dataset, raw=dataset_raw, require_coverage=args.set == "tuning")
        if args.limit is not None:
            contract.require(args.limit == len(samples), "partial_dataset_forbidden")
        if args.set == "heldout":
            heldout.assert_separate(samples, read_dataset(DATASET, require_coverage=False))
            gaps = heldout.coverage_gaps(samples)
            contract.require(not gaps, "heldout_coverage_missing")
        question_raw = contract.bounded_file(args.question_manifest, contract.MAX_REQUEST) if args.question_manifest else \
            contract.encode(default_questions(args.arm, criteria))
        manifest = contract.Manifest(args.source_sha, criteria_raw, dataset_raw, question_raw, args.endpoint, args.model)
        transport = JevTransport()
        token = load_token(args.token_file)
        result = evaluate(samples, arm=args.arm, criteria=criteria, token=token, endpoint=args.endpoint,
                          model=args.model, timeout=args.timeout, manifest=manifest, transport=transport,
                          dataset_set=args.set)
        result["set"], result["dataset"] = args.set, str(args.dataset)
        if args.set == "heldout":
            result["recovery_qualification"] = heldout.qualification(samples, result["predictions"], complete_dataset=True)
        result["criteria_mode"] = args.criteria_mode
        result["dataset_slice"] = {"offset": 0, "count": len(samples), "complete_dataset": True}
    except (OSError, ValueError) as error:
        code = getattr(error, "code", "input_preflight")
        print(f"tab-state Jev evaluation: {code}", file=sys.stderr)
        return 2
    print(json.dumps(result, ensure_ascii=False, indent=2, allow_nan=False))
    return 0 if result["successful_calls"] == len(samples) else 1


if __name__ == "__main__":
    raise SystemExit(main())
