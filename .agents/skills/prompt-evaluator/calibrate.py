"""Judge calibration mode.

Hand-labeled `golden.jsonl` next to each `evals.json` records what a
HUMAN says about a frozen agent response, per-expectation. We replay
those frozen responses through the LLM judge and compute Cohen's κ to
measure how close the judge tracks the human.

Without this: every change to JUDGE_PROMPT_TEMPLATE / judge model is
flying blind — the run might pass more/fewer evals because the judge
got better, or because it got LAXER. κ separates the two.

`golden.jsonl` schema (one JSON object per line):

    {
      "eval_id": 1,
      "response": "<frozen agent command-list response — paste verbatim>",
      "notes": "<optional: why this response, what it tests>",
      "labels": [
        {"expectation_idx": 0, "verdict": "PASS"},
        {"expectation_idx": 1, "verdict": "FAIL"},
        ...
      ]
    }

`expectation_idx` is the 0-based index into the eval's `expectations`
array in evals.json — labels are anchored to position so re-ordering
expectations breaks them (intentional: forces re-labeling on change).

How to grow a golden set:

1. Run the eval once normally with --verbose; copy 5-10 representative
   agent responses (mix of clean PASS, clean FAIL, ambiguous edge cases).
2. For each, hand-label every expectation as PASS or FAIL.
3. `python runner.py --calibrate-judge --eval-set evals/<set>/evals.json`
   prints per-expectation κ + a summary κ.

Cohen's κ interpretation (Landis & Koch 1977):
- < 0.20 = poor
- 0.21-0.40 = fair
- 0.41-0.60 = moderate
- 0.61-0.80 = substantial
- > 0.80 = almost perfect (target)

Below substantial (κ < 0.6), don't trust judge verdicts on this
eval-set without re-checking template / judge model.
"""
from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any


@dataclass
class GoldenLabel:
    eval_id: int
    response: str
    expectation_idx: int
    human_verdict: bool   # True = PASS, False = FAIL
    notes: str = ""


def load_golden(eval_set_path: str) -> list[GoldenLabel]:
    """
    Look for `golden.jsonl` next to the eval-set file. Return [] if absent
    (calibration is opt-in).
    """
    p = Path(eval_set_path).parent / "golden.jsonl"
    if not p.exists():
        return []
    out: list[GoldenLabel] = []
    for line_no, raw in enumerate(p.read_text(encoding="utf-8").splitlines(), 1):
        raw = raw.strip()
        if not raw or raw.startswith("//") or raw.startswith("#"):
            continue
        try:
            entry = json.loads(raw)
        except json.JSONDecodeError as e:
            raise ValueError(f"{p}:{line_no}: invalid JSON — {e}") from e
        eid = entry["eval_id"]
        resp = entry["response"]
        notes = entry.get("notes", "")
        for lab in entry.get("labels", []):
            v = lab["verdict"].upper()
            if v not in ("PASS", "FAIL"):
                raise ValueError(f"{p}:{line_no}: verdict must be PASS or FAIL, got {v!r}")
            out.append(GoldenLabel(
                eval_id=eid,
                response=resp,
                expectation_idx=lab["expectation_idx"],
                human_verdict=(v == "PASS"),
                notes=notes,
            ))
    return out


def cohens_kappa(pairs: list[tuple[bool, bool]]) -> float:
    """
    Cohen's κ for binary classification.
    pairs: list of (human, judge) — both True/False.
    Returns 1.0 for perfect agreement, 0 for chance, negative for systematic disagreement.
    """
    n = len(pairs)
    if n == 0:
        return 0.0
    a = sum(1 for h, j in pairs if h == j) / n  # observed agreement
    p_h = sum(1 for h, _ in pairs if h) / n
    p_j = sum(1 for _, j in pairs if j) / n
    e = p_h * p_j + (1 - p_h) * (1 - p_j)        # chance agreement
    if abs(1 - e) < 1e-9:
        return 1.0 if a >= 1 - 1e-9 else 0.0
    return (a - e) / (1 - e)


def kappa_label(k: float) -> str:
    if k < 0.2: return "poor"
    if k < 0.4: return "fair"
    if k < 0.6: return "moderate"
    if k < 0.8: return "substantial"
    return "almost-perfect"


@dataclass
class CalibrationReport:
    overall_kappa: float
    per_expectation: dict[tuple[int, int], float]   # (eval_id, exp_idx) -> κ
    pair_count: int
    agreement_count: int

    def render(self, evals: dict[int, list[str]]) -> str:
        lines = [
            "# Judge calibration report",
            "",
            f"- Total labeled (response, expectation) pairs: **{self.pair_count}**",
            f"- Agreement: {self.agreement_count}/{self.pair_count} "
            f"({100 * self.agreement_count // max(1, self.pair_count)}%)",
            f"- Cohen's κ overall: **{self.overall_kappa:+.3f}** ({kappa_label(self.overall_kappa)})",
            "",
            "## Per-expectation",
            "",
            "| eval_id | exp_idx | expectation | κ | tier |",
            "|---------|---------|-------------|---|------|",
        ]
        for (eid, idx), k in sorted(self.per_expectation.items()):
            exps = evals.get(eid, [])
            exp_text = exps[idx][:80] if idx < len(exps) else "?"
            lines.append(f"| {eid} | {idx} | {exp_text} | {k:+.3f} | {kappa_label(k)} |")
        lines.append("")
        lines.append(
            "Target overall κ ≥ 0.6 (substantial). Below that, the judge "
            "isn't reliably tracking human verdicts — reconsider the template, "
            "swap to a stronger judge model (sonnet > haiku), or rewrite the "
            "ambiguous expectations to be more specific."
        )
        return "\n".join(lines)


def calibrate(
    golden: list[GoldenLabel],
    evals: list[Any],   # list of Eval (loaded from runner.load_eval_set)
    judge_model: str,
    judge_concurrency: int = 4,
) -> CalibrationReport:
    """
    For each (frozen response, expectation) pair, run the LLM judge
    and compare to the human label. Cohen's κ on the result.
    """
    from concurrent.futures import ThreadPoolExecutor
    from matcher import match_expectation

    eval_by_id = {e.id: e for e in evals}

    def one_pair(label: GoldenLabel) -> tuple[GoldenLabel, bool]:
        ev = eval_by_id.get(label.eval_id)
        if ev is None or label.expectation_idx >= len(ev.expectations):
            return (label, False)
        result = match_expectation(
            ev.expectations[label.expectation_idx],
            label.response,
            judge_model=judge_model,
        )
        return (label, result.passed)

    judge_results: list[tuple[GoldenLabel, bool]] = []
    with ThreadPoolExecutor(max_workers=max(1, judge_concurrency)) as pool:
        for r in pool.map(one_pair, golden):
            judge_results.append(r)

    pairs = [(lab.human_verdict, judge_v) for lab, judge_v in judge_results]
    overall = cohens_kappa(pairs)

    by_exp: dict[tuple[int, int], list[tuple[bool, bool]]] = {}
    for lab, judge_v in judge_results:
        by_exp.setdefault((lab.eval_id, lab.expectation_idx), []).append(
            (lab.human_verdict, judge_v)
        )
    per_exp = {key: cohens_kappa(p) for key, p in by_exp.items()}

    return CalibrationReport(
        overall_kappa=overall,
        per_expectation=per_exp,
        pair_count=len(pairs),
        agreement_count=sum(1 for h, j in pairs if h == j),
    )
