"""Qualification of newly captured human-labelled recovery frames."""
from collections import Counter
import hashlib

AUTOMATIC_LABELS = ("provider_update_prompt", "crashed_to_shell", "still_starting", "error_loop")
NOTIFICATION_LABELS = ("trust_or_permission_prompt", "login_or_auth_required", "model_unavailable_or_quota")
NEGATIVES = ("ready_idle", "working_normally", "other")
AUTOMATIC_THRESHOLD = 0.90
OWNER_NOTIFICATION_THRESHOLD = 0.80
REQUIRED_PRECISION = 0.95


def screen_hash(sample):
    return hashlib.sha256(sample["screen"].replace("\r\n", "\n").rstrip().encode()).hexdigest()


def coverage_gaps(samples):
    counts = Counter(sample["label"] for sample in samples)
    return [f"{label}={counts[label]}" for label in AUTOMATIC_LABELS if counts[label] < 4] + [
        f"{label}=0" for label in NEGATIVES if not counts[label]
    ]


def assert_separate(samples, tuning):
    tuning_ids = {row["id"] for row in tuning}
    tuning_hashes = {screen_hash(row) for row in tuning}
    hashes = set()
    for sample in samples:
        fingerprint = screen_hash(sample)
        if sample["id"] in tuning_ids or fingerprint in tuning_hashes:
            raise ValueError(f"held-out frame {sample['id']} overlaps tuning frames")
        if fingerprint in hashes:
            raise ValueError(f"duplicate held-out screen {sample['id']}")
        hashes.add(fingerprint)


def band_metrics(predictions, labels, threshold):
    chosen = [p for p in predictions if p.get("prediction") in labels and p.get("confidence", 0) >= threshold]
    correct = sum(p["prediction"] == p["label"] for p in chosen)
    per_label = {}
    for label in labels:
        matching = [p for p in chosen if p["prediction"] == label]
        right = sum(p["label"] == label for p in matching)
        per_label[label] = {"predicted": len(matching), "correct": right, "precision": right / len(matching) if matching else None}
    return {"threshold": threshold, "predicted": len(chosen), "correct": correct,
            "precision": correct / len(chosen) if chosen else None, "per_label": per_label}


def qualification(samples, predictions, *, complete_dataset=True):
    automatic = band_metrics(predictions, AUTOMATIC_LABELS, AUTOMATIC_THRESHOLD)
    notifications = band_metrics(predictions, NOTIFICATION_LABELS, OWNER_NOTIFICATION_THRESHOLD)
    gaps = coverage_gaps(samples)
    all_answered = len(predictions) == len(samples) and all("prediction" in p for p in predictions)
    eligible = complete_dataset and not gaps and all_answered and automatic["predicted"] >= 16
    passed = eligible and automatic["precision"] is not None and automatic["precision"] >= REQUIRED_PRECISION
    return {"automatic_band": automatic, "notification_band": notifications,
            "human_labelled_support": dict(Counter(sample["label"] for sample in samples)),
            "eligible": eligible, "coverage_gaps": gaps, "required_precision": REQUIRED_PRECISION,
            "decision": "qualifies_for_live_oracle" if passed else "notify_only",
            "automatic_actions_enabled": False}
