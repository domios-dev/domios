"""Pure ranking and gate metrics for the live memory evaluation."""

from __future__ import annotations

from collections.abc import Iterable, Sequence


def first_relevant_rank(hit_ids: Sequence[str], answer_ids: Iterable[str]) -> int | None:
    answers = set(answer_ids)
    return next((rank for rank, item_id in enumerate(hit_ids, 1) if item_id in answers), None)


def reciprocal_rank(rank: int | None, cutoff: int = 10) -> float:
    if rank is None or rank < 1 or rank > cutoff:
        return 0.0
    return 1.0 / rank


def gate_counts(
    scores: Sequence[float | None], absent_labels: Sequence[bool], threshold: float
) -> dict[str, int | float | None]:
    if len(scores) != len(absent_labels):
        raise ValueError("scores and labels must have equal length")
    predicted = [score is not None and score <= threshold for score in scores]
    true_positive = sum(gate and absent for gate, absent in zip(predicted, absent_labels))
    false_positive = sum(gate and not absent for gate, absent in zip(predicted, absent_labels))
    false_negative = sum(not gate and absent for gate, absent in zip(predicted, absent_labels))
    predicted_count = true_positive + false_positive
    absent_count = sum(absent_labels)
    return {
        "true_positive": true_positive,
        "false_positive": false_positive,
        "false_negative": false_negative,
        "predicted_gate": predicted_count,
        "absent_count": absent_count,
        "precision": true_positive / predicted_count if predicted_count else None,
        "recall": true_positive / absent_count if absent_count else None,
    }
