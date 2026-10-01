#!/usr/bin/env python3
"""Measure screen-only and context-aware tab-state classification."""

from __future__ import annotations

import argparse
import json
import math
import os
import ssl
import statistics
import sys
import time
import urllib.error
import urllib.request
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

try:
    import certifi

    SSL_CONTEXT = ssl.create_default_context(cafile=certifi.where())
except ImportError:
    SSL_CONTEXT = ssl.create_default_context()


ROOT = Path(__file__).resolve().parents[2]
DATASET = Path(__file__).with_name("screens.jsonl")
CRITERIA_FILE = Path(__file__).with_name("criteria_variants.json")
DEFAULT_ENDPOINT = "https://api.typesafe.ai/v1/systemone"
DEFAULT_MODEL = "jev-latest"
MAX_CALLS_PER_RUN = 300
STRONG_THRESHOLD = 0.80
WEAK_THRESHOLD = 0.60
STRONG_GATE = 0.90
WEAK_GATE = 0.60
MIN_FRAMES_PER_GROUP = 4
REQUIRED_PROVIDERS = ("claude", "codex", "opencode", "cursor")
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
    support = Counter((sample["provider"], sample["label"]) for sample in samples)
    gaps = [
        f"{provider}/{label}={support[(provider, label)]}"
        for provider in REQUIRED_PROVIDERS
        for label in LABELS
        if support[(provider, label)] < MIN_FRAMES_PER_GROUP
    ]
    lm_studio = [
        sample
        for sample in samples
        if sample["transport"] == "chat_backend"
        and sample["provider_capability"]["display_name"].strip().casefold() == "lm studio"
    ]
    chat_support = Counter(sample["label"] for sample in lm_studio)
    gaps.extend(
        f"LM Studio/chat_backend/{label}={chat_support[label]}"
        for label in LABELS
        if chat_support[label] < MIN_FRAMES_PER_GROUP
    )
    return gaps


def read_dataset(path: Path) -> list[dict[str, Any]]:
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
    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip():
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError as error:
            raise ValueError(f"invalid JSON on {path}:{line_number}: {error.msg}") from error
        if not isinstance(row, dict) or not required <= row.keys():
            raise ValueError(f"{path}:{line_number} is missing required fields")
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
    if gaps:
        raise ValueError(
            f"the labelled set needs at least {MIN_FRAMES_PER_GROUP} real frames per label/provider; "
            f"missing: {', '.join(gaps)}"
        )
    return rows


def load_criteria(path: Path, variant: str) -> dict[str, str]:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"criteria variants are unreadable: {error}") from error
    variants = data.get("variants") if isinstance(data, dict) else None
    criteria = variants.get(variant) if isinstance(variants, dict) else None
    if not isinstance(criteria, dict) or set(criteria) != set(LABELS):
        raise ValueError(f"criteria variant {variant!r} does not define every state label")
    if any(not isinstance(value, str) or not value.strip() for value in criteria.values()):
        raise ValueError(f"criteria variant {variant!r} contains an empty criterion")
    return criteria


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
            "for the named dev instance"
        ) from error
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError("Jev token file is unreadable or invalid") from error
    token = value.get("token") if isinstance(value, dict) else None
    if not isinstance(token, str) or not token.strip():
        raise ValueError("Jev token file has no nonempty token field")
    return token.strip()


def request_body(
    screen: str,
    context: dict[str, Any],
    *,
    arm: str,
    model: str,
    criteria: dict[str, str],
) -> bytes:
    state: Any = screen if arm == "screen" else {"screen": screen, "context": context}
    body = {
        "state": state,
        "model": model,
        "questions": {
            "q": {
                "type": "choice",
                "instructions": INSTRUCTIONS,
                "criteria": criteria,
            }
        },
    }
    return json.dumps(body, ensure_ascii=False).encode("utf-8")


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
) -> tuple[dict[str, Any] | None, float, str | None]:
    request = urllib.request.Request(
        endpoint,
        data=request_body(screen, context, arm=arm, model=model, criteria=criteria),
        headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"},
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
    if not isinstance(probabilities, dict) or any(
        key not in LABELS
        or isinstance(value, bool)
        or not isinstance(value, (int, float))
        or not math.isfinite(value)
        or not 0 <= value <= 1
        for key, value in probabilities.items()
    ):
        return None, elapsed, "invalid_probabilities"
    confidence = answer.get("confidence")
    source = "reported"
    if confidence is None:
        confidence = probabilities.get(choice)
        source = "choice_probability"
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
        "confidence_source": source,
    }, elapsed, None


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
) -> dict[str, Any]:
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

    successful = [item for item in predictions if "prediction" in item]
    bands = band_metrics(samples, successful)
    return {
        "arm": arm,
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
        "gate": gate_verdict(
            bands["strong_actionable"]["precision"],
            bands["weak"]["precision"],
        ),
        "latency_ms": {
            "mean": statistics.fmean(latencies) if latencies else None,
            "p50": nearest_rank(latencies, 0.50),
            "p95": nearest_rank(latencies, 0.95),
            "n": len(latencies),
        },
        "predictions": predictions,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dataset", type=Path, default=DATASET)
    parser.add_argument("--criteria-file", type=Path, default=CRITERIA_FILE)
    parser.add_argument("--criteria-variant", choices=("current", "auth-first-run"), default="current")
    parser.add_argument("--arm", choices=("screen", "context"), default="screen")
    parser.add_argument("--token-file", type=Path)
    parser.add_argument("--endpoint", default=os.environ.get("JEV_ENDPOINT", DEFAULT_ENDPOINT))
    parser.add_argument("--model", default=os.environ.get("JEV_MODEL", DEFAULT_MODEL))
    parser.add_argument("--offset", type=int, default=0)
    parser.add_argument("--limit", type=int)
    parser.add_argument("--timeout", type=float, default=60.0)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.offset < 0:
        raise SystemExit("--offset must be nonnegative")
    if args.limit is not None and args.limit < 1:
        raise SystemExit("--limit must be positive")
    if args.timeout <= 0:
        raise SystemExit("--timeout must be positive")
    try:
        all_samples = read_dataset(args.dataset)
        criteria = load_criteria(args.criteria_file, args.criteria_variant)
        samples = all_samples[args.offset :]
        if args.limit is not None:
            samples = samples[: args.limit]
        if not samples:
            raise ValueError("the requested dataset slice has no records")
        if len(samples) > MAX_CALLS_PER_RUN:
            raise ValueError(
                f"{len(samples)} records exceed the hard {MAX_CALLS_PER_RUN}-call run ceiling; "
                "use --offset and --limit to evaluate slices"
            )
        token = load_token(args.token_file)
        result = evaluate(
            samples,
            arm=args.arm,
            criteria=criteria,
            token=token,
            endpoint=args.endpoint,
            model=args.model,
            timeout=args.timeout,
        )
        result["criteria_variant"] = args.criteria_variant
        result["dataset_slice"] = {
            "offset": args.offset,
            "count": len(samples),
            "complete_dataset": args.offset == 0 and len(samples) == len(all_samples),
        }
    except (OSError, ValueError) as error:
        print(f"tab-state Jev evaluation: {error}", file=sys.stderr)
        return 2
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return 0 if result["successful_calls"] == len(samples) else 1


if __name__ == "__main__":
    raise SystemExit(main())
