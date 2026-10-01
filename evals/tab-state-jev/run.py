#!/usr/bin/env python3
"""Classify fixed terminal screens with Jev and compare recorded structural outputs."""

from __future__ import annotations

import argparse
import json
import math
import os
import statistics
import sys
import time
import ssl
import urllib.error
import urllib.request
from collections import Counter
from pathlib import Path
from typing import Any

try:
    import certifi

    SSL_CONTEXT = ssl.create_default_context(cafile=certifi.where())
except ImportError:
    SSL_CONTEXT = ssl.create_default_context()


ROOT = Path(__file__).resolve().parents[2]
DATASET = Path(__file__).with_name("screens.jsonl")
DEFAULT_ENDPOINT = "https://api.typesafe.ai/v1/systemone"
DEFAULT_MODEL = "jev-latest"
DEFAULT_MAX_CALLS = 300
CONFIDENCE_BAR = 0.8
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
)
ACTIONABLE = {
    "provider_update_prompt",
    "trust_or_permission_prompt",
    "login_or_auth_required",
    "model_unavailable_or_quota",
    "crashed_to_shell",
    "error_loop",
}
READY = {"ready_idle", "working_normally"}
CRITERIA = {
    "ready_idle": "The agent TUI is ready, and its composer is waiting for user input.",
    "working_normally": "The agent TUI is making ordinary progress, such as thinking or running tools.",
    "provider_update_prompt": "The provider is blocked on an update available dialog or update choice.",
    "trust_or_permission_prompt": "The provider is blocked on a trust, folder access, command approval, or permission choice.",
    "login_or_auth_required": "The provider requires sign-in, authentication, or an authorization step.",
    "model_unavailable_or_quota": "The selected model is unavailable or the provider reached a usage quota or rate limit.",
    "crashed_to_shell": "The provider TUI is gone and the terminal is showing a shell prompt.",
    "error_loop": "The provider repeatedly shows the same runtime or tool error and makes no progress.",
    "still_starting": "The provider is initializing and has not reached a usable agent TUI state.",
}
INSTRUCTIONS = "Classify the current agent tab state from its visible terminal screen. Choose the single best label."


def read_dataset(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip():
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError as error:
            raise ValueError(f"invalid JSON on {path}:{line_number}: {error.msg}") from error
        required = {
            "id",
            "provider",
            "boot_stage",
            "boot_stage_source",
            "label",
            "source",
            "screen",
            "baselines",
        }
        if not required <= row.keys():
            raise ValueError(f"{path}:{line_number} is missing required fields")
        if row["label"] not in LABELS or not isinstance(row["screen"], str) or not row["screen"].strip():
            raise ValueError(f"{path}:{line_number} has an invalid label or empty screen")
        rows.append(row)
    if len(rows) < 60:
        raise ValueError(f"the fixed dataset has {len(rows)} screens; at least 60 are required")
    ids = [row["id"] for row in rows]
    if len(set(ids)) != len(ids):
        raise ValueError("the fixed dataset contains duplicate ids")
    return rows


def load_token(token_file: Path | None) -> str:
    for name in ("JEV_API_TOKEN", "TYPESAFE_JEV_TOKEN"):
        token = os.environ.get(name, "").strip()
        if token:
            return token
    path = token_file or Path.home() / ".codeterm-dev" / "decision-models" / "typesafe-jev.json"
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as error:
        raise ValueError(
            "Jev token missing; set JEV_API_TOKEN in the parent environment or pass --token-file "
            "for the dev token store"
        ) from error
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError("Jev token file is unreadable or invalid") from error
    token = value.get("token") if isinstance(value, dict) else None
    if not isinstance(token, str) or not token.strip():
        raise ValueError("Jev token file has no nonempty token field")
    return token.strip()


def request_body(screen: str, model: str) -> bytes:
    body = {
        "state": screen,
        "model": model,
        "questions": {
            "q": {
                "type": "choice",
                "instructions": INSTRUCTIONS,
                "criteria": CRITERIA,
            }
        },
    }
    return json.dumps(body, ensure_ascii=False).encode("utf-8")


def call_jev(
    screen: str,
    *,
    token: str,
    endpoint: str,
    model: str,
    timeout: float,
) -> tuple[dict[str, Any] | None, float, str | None]:
    request = urllib.request.Request(
        endpoint,
        data=request_body(screen, model),
        headers={
            "Authorization": f"Bearer {token}",
            "Content-Type": "application/json",
        },
        method="POST",
    )
    started = time.perf_counter()
    try:
        with urllib.request.urlopen(request, timeout=timeout, context=SSL_CONTEXT) as response:
            payload = json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        return None, (time.perf_counter() - started) * 1000, f"http_{error.code}"
    except (urllib.error.URLError, TimeoutError, OSError) as error:
        reason = getattr(error, "reason", error)
        return None, (time.perf_counter() - started) * 1000, f"transport_error: {type(reason).__name__}"
    except (UnicodeDecodeError, json.JSONDecodeError):
        return None, (time.perf_counter() - started) * 1000, "invalid_json"
    elapsed = (time.perf_counter() - started) * 1000
    answers = payload.get("answers") if isinstance(payload, dict) else None
    answer = answers.get("q") if isinstance(answers, dict) else None
    if not isinstance(answer, dict) or answer.get("type") != "choice":
        return None, elapsed, "invalid_answer_shape"
    choice = answer.get("choice")
    if choice not in LABELS:
        return None, elapsed, "invalid_choice"
    probabilities = answer.get("probabilities")
    if not isinstance(probabilities, dict) or not probabilities:
        return None, elapsed, "invalid_probabilities"
    if any(
        key not in LABELS
        or isinstance(probability, bool)
        or not isinstance(probability, (int, float))
        or not math.isfinite(probability)
        or not 0 <= probability <= 1
        for key, probability in probabilities.items()
    ):
        return None, elapsed, "invalid_probabilities"
    confidence = answer.get("confidence")
    confidence_source = "reported"
    if confidence is None:
        confidence = probabilities.get(choice)
        confidence_source = "choice_probability"
    if (
        isinstance(confidence, bool)
        or not isinstance(confidence, (int, float))
        or not math.isfinite(confidence)
        or not 0 <= confidence <= 1
    ):
        return None, elapsed, "invalid_confidence"
    return {
        "prediction": choice,
        "confidence": float(confidence),
        "confidence_source": confidence_source,
    }, elapsed, None


def nearest_rank(values: list[float], percentile: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    index = max(0, math.ceil(percentile * len(ordered)) - 1)
    return ordered[index]


def confusion_matrix(rows: list[tuple[str, str]]) -> dict[str, dict[str, int]]:
    counts: Counter[tuple[str, str]] = Counter(rows)
    return {
        actual: {predicted: counts[(actual, predicted)] for predicted in LABELS}
        for actual in LABELS
    }


def classification_metrics(rows: list[tuple[str, str]]) -> dict[str, Any]:
    matrix = confusion_matrix(rows)
    metrics: dict[str, Any] = {}
    recalls: list[float] = []
    f1s: list[float] = []
    for label in LABELS:
        support = sum(matrix[label].values())
        predicted = sum(matrix[actual][label] for actual in LABELS)
        true_positive = matrix[label][label]
        precision = true_positive / predicted if predicted else None
        recall = true_positive / support if support else None
        f1 = (
            2 * precision * recall / (precision + recall)
            if precision is not None and recall is not None and precision + recall
            else None
        )
        if recall is not None:
            recalls.append(recall)
        if f1 is not None:
            f1s.append(f1)
        metrics[label] = {
            "support": support,
            "predicted": predicted,
            "precision": precision,
            "recall": recall,
            "f1": f1,
        }
    count = len(rows)
    supported = sum(item["support"] > 0 for item in metrics.values())
    return {
        "count": count,
        "accuracy": sum(matrix[label][label] for label in LABELS) / count if count else None,
        "macro_accuracy": statistics.fmean(recalls) if recalls else None,
        "macro_f1": statistics.fmean(f1s) if f1s else None,
        "represented_labels": supported,
        "per_label": metrics,
        "confusion_matrix": matrix,
    }


def high_confidence_metrics(predictions: list[dict[str, Any]]) -> dict[str, Any]:
    selected = [item for item in predictions if item.get("confidence") is not None and item["confidence"] >= CONFIDENCE_BAR]
    actionable = [item for item in selected if item["prediction"] in ACTIONABLE]
    correct_actionable = sum(item["prediction"] == item["label"] for item in actionable)
    per_actionable: dict[str, Any] = {}
    for label in sorted(ACTIONABLE):
        predicted = [item for item in selected if item["prediction"] == label]
        correct = sum(item["label"] == label for item in predicted)
        per_actionable[label] = {
            "predicted_at_threshold": len(predicted),
            "correct": correct,
            "precision": correct / len(predicted) if predicted else None,
        }
    return {
        "threshold": CONFIDENCE_BAR,
        "count": len(selected),
        "accuracy": sum(item["prediction"] == item["label"] for item in selected) / len(selected)
        if selected
        else None,
        "coverage": len(selected) / len(predictions) if predictions else None,
        "actionable_predictions": len(actionable),
        "actionable_precision": correct_actionable / len(actionable) if actionable else None,
        "actionable_per_label": per_actionable,
    }


def structural_comparison(
    samples: list[dict[str, Any]], prediction_by_id: dict[str, dict[str, Any]]
) -> dict[str, Any]:
    detector_rows: dict[str, list[tuple[dict[str, Any], str | None, str]]] = {}
    raw_outputs: dict[str, Counter[str]] = {}
    for sample in samples:
        for detector, output in sample.get("baselines", {}).items():
            raw = output.get("raw", "unknown") if isinstance(output, dict) else "unknown"
            raw_outputs.setdefault(detector, Counter())[str(raw)] += 1
            if isinstance(output, dict):
                detector_rows.setdefault(detector, []).append(
                    (sample, output.get("label"), str(raw))
                )
    detectors: dict[str, Any] = {}
    for detector, rows in detector_rows.items():
        comparable = [(sample, label) for sample, label, _ in rows if label in LABELS]
        legacy_pairs = [(sample["label"], label) for sample, label in comparable]
        jev_pairs = [
            (sample["label"], prediction_by_id[sample["id"]]["prediction"])
            for sample, _ in comparable
            if sample["id"] in prediction_by_id
        ]
        same_rows = [
            (sample, label)
            for sample, label in comparable
            if sample["id"] in prediction_by_id
        ]
        detectors[detector] = {
            "computed_rows": len(rows),
            "labelled_output_rows": len(comparable),
            "abstentions_or_nonmapped_outputs": len(rows) - len(comparable),
            "coverage": len(comparable) / len(samples) if samples else None,
            "legacy": classification_metrics(legacy_pairs),
            "jev_on_same_rows": classification_metrics(jev_pairs),
            "jev_agreement_with_legacy": sum(
                prediction_by_id[sample["id"]]["prediction"] == label
                for sample, label in same_rows
            )
            / len(same_rows)
            if same_rows
            else None,
            "raw_outputs": dict(sorted(raw_outputs[detector].items())),
        }
    stage_rows = [sample for sample in samples if sample.get("boot_stage")]
    stage_breakdown: dict[str, Counter[str]] = {}
    for sample in stage_rows:
        stage_breakdown.setdefault(sample["boot_stage"], Counter())[sample["label"]] += 1
    return {
        "detectors": detectors,
        "boot_stage": {
            "recorded_rows": len(stage_rows),
            "coverage": len(stage_rows) / len(samples) if samples else None,
            "by_stage_and_screen_label": {
                stage: dict(sorted(counts.items())) for stage, counts in sorted(stage_breakdown.items())
            },
        },
    }


def readiness_precision(predictions: list[dict[str, Any]]) -> dict[str, Any]:
    ready = [
        item
        for item in predictions
        if item["confidence"] >= CONFIDENCE_BAR and item["prediction"] in READY
    ]
    correct = sum(item["label"] in READY for item in ready)
    return {
        "predicted_ready_at_threshold": len(ready),
        "precision": correct / len(ready) if ready else None,
    }


def verdicts(
    samples: list[dict[str, Any]],
    predictions: list[dict[str, Any]],
    calibration: dict[str, Any],
) -> dict[str, Any]:
    support = Counter(sample["label"] for sample in samples)
    missing = [label for label in LABELS if not support[label]]
    action_labels_pass = all(
        calibration["actionable_per_label"][label]["precision"] is not None
        and calibration["actionable_per_label"][label]["precision"] >= 0.90
        for label in ACTIONABLE
    )
    full_label_coverage = not missing
    ready_precision = readiness_precision(predictions)
    confident_count = calibration["count"]
    if full_label_coverage and action_labels_pass and (ready_precision["precision"] or 0) >= 0.90:
        spawn = "hint"
        spawn_reason = "All nine labels are represented; every actionable label clears 90% precision at confidence 0.8, and ready-state precision clears 90%."
    elif full_label_coverage and confident_count:
        spawn = "shadow"
        spawn_reason = "All labels are represented, but the proposed high-confidence precision bar is not met."
    else:
        spawn = "nothing"
        spawn_reason = "The corpus lacks required state coverage, so a spawn-health decision is not measured."
    stuck_precision = [
        calibration["actionable_per_label"][label]["precision"]
        for label in ("crashed_to_shell", "error_loop")
    ]
    if full_label_coverage and action_labels_pass and all(value is not None and value >= 0.90 for value in stuck_precision):
        stuck = "hint"
        stuck_reason = "Both shell-crash and error-loop precision clear 90% at confidence 0.8, with full label coverage."
    elif full_label_coverage and confident_count:
        stuck = "shadow"
        stuck_reason = "All labels are represented, but the proposed stuck-state precision bar is not met."
    else:
        stuck = "nothing"
        stuck_reason = "The corpus lacks error-loop coverage, so the stuck-working decision is not measured."
    return {
        "proposed_actionable_precision_bar": 0.90,
        "spawn_health": {"verdict": spawn, "reason": spawn_reason, "ready_precision_at_0_8": ready_precision},
        "stuck_working": {"verdict": stuck, "reason": stuck_reason},
        "missing_ground_truth_labels": missing,
        "label_support": {label: support[label] for label in LABELS},
        "action_labels_clear_bar": action_labels_pass,
        "confidence_0_8_predictions": confident_count,
    }


def evaluate(
    samples: list[dict[str, Any]],
    *,
    token: str,
    endpoint: str,
    model: str,
    timeout: float,
) -> dict[str, Any]:
    predictions: list[dict[str, Any]] = []
    latencies: list[float] = []
    failures: Counter[str] = Counter()
    for sample in samples:
        answer, latency_ms, error = call_jev(
            sample["screen"],
            token=token,
            endpoint=endpoint,
            model=model,
            timeout=timeout,
        )
        latencies.append(latency_ms)
        record: dict[str, Any] = {
            "id": sample["id"],
            "label": sample["label"],
            "latency_ms": round(latency_ms, 2),
        }
        if error:
            record["error"] = error
            failures[error] += 1
        else:
            record.update(answer or {})
        predictions.append(record)

    successful = [item for item in predictions if "prediction" in item]
    pairs = [(item["label"], item["prediction"]) for item in successful]
    metrics = classification_metrics(pairs)
    calibration = high_confidence_metrics(successful)
    prediction_by_id = {item["id"]: item for item in successful}
    return {
        "model": model,
        "endpoint": endpoint,
        "dataset": str(DATASET.relative_to(ROOT)),
        "snapshot_count": len(samples),
        "successful_calls": len(successful),
        "failed_calls": dict(sorted(failures.items())),
        "metrics": metrics,
        "calibration_at_0_8": calibration,
        "latency_ms": {
            "mean": statistics.fmean(latencies) if latencies else None,
            "p50": nearest_rank(latencies, 0.50),
            "p95": nearest_rank(latencies, 0.95),
            "n": len(latencies),
        },
        "structural_comparison": structural_comparison(samples, prediction_by_id),
        "verdicts": verdicts(samples, successful, calibration),
        "predictions": predictions,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dataset", type=Path, default=DATASET)
    parser.add_argument("--token-file", type=Path)
    parser.add_argument("--endpoint", default=os.environ.get("JEV_ENDPOINT", DEFAULT_ENDPOINT))
    parser.add_argument("--model", default=os.environ.get("JEV_MODEL", DEFAULT_MODEL))
    parser.add_argument("--limit", type=int)
    parser.add_argument("--max-calls", type=int, default=DEFAULT_MAX_CALLS)
    parser.add_argument("--timeout", type=float, default=60.0)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if not 1 <= args.max_calls <= DEFAULT_MAX_CALLS:
        raise SystemExit(f"--max-calls must be between 1 and {DEFAULT_MAX_CALLS}")
    if args.limit is not None and args.limit < 1:
        raise SystemExit("--limit must be positive")
    if args.timeout <= 0:
        raise SystemExit("--timeout must be positive")
    try:
        samples = read_dataset(args.dataset)
        if args.limit is not None:
            samples = samples[: args.limit]
        if len(samples) > args.max_calls:
            raise ValueError(
                f"{len(samples)} screens require more than the configured {args.max_calls} Jev calls"
            )
        token = load_token(args.token_file)
        result = evaluate(
            samples,
            token=token,
            endpoint=args.endpoint,
            model=args.model,
            timeout=args.timeout,
        )
    except (OSError, ValueError) as error:
        print(f"tab-state Jev evaluation: {error}", file=sys.stderr)
        return 2
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return 0 if result["successful_calls"] == len(samples) else 1


if __name__ == "__main__":
    raise SystemExit(main())
