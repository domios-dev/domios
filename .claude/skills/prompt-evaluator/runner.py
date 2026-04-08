#!/usr/bin/env python3
"""
prompt-evaluator runner.

Spawns the local `claude` CLI in non-interactive sandbox mode for each
eval prompt, captures the agent's response, runs the hybrid expectation
matcher, and produces a markdown report. Supports A/B comparison between
two prompt-file revisions.

Usage:
    # Single run on the current prompt
    python runner.py \\
      --prompt-file src-tauri/prompts/general_agent.md \\
      --eval-set evals/general-agent/evals.json \\
      --runs 3

    # A/B compare HEAD vs HEAD~1 of the same file
    python runner.py \\
      --prompt-a HEAD~1:src-tauri/prompts/general_agent.md \\
      --prompt-b src-tauri/prompts/general_agent.md \\
      --eval-set evals/general-agent/evals.json \\
      --runs 3 \\
      --report markdown

    # Debug a single eval
    python runner.py \\
      --prompt-file src-tauri/prompts/general_agent.md \\
      --eval-set evals/general-agent/evals.json \\
      --only-id 8 --runs 1 --verbose

The runner exits with code 0 if all evals pass (single mode) or no
regressions are found (A/B mode), and 1 otherwise.
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any


def _force_rmtree(path: str) -> None:
    """
    Best-effort recursive removal that swallows ALL errors. Needed because
    Python's `tempfile.TemporaryDirectory` (even with ignore_cleanup_errors)
    fails on Windows when bash scripts inside the temp dir create literal
    files named 'NUL', 'CON', 'PRN', etc — those are Windows reserved device
    names and Python's chmod-then-unlink cleanup chain raises uncatchable
    errors. We don't care about a few KB of leftover hook curl output;
    %TEMP% gets purged on system maintenance.
    """
    def _swallow(func, path, exc):  # noqa: ARG001
        pass

    shutil.rmtree(path, onerror=_swallow)

# Local import — matcher.py lives next to this file
sys.path.insert(0, str(Path(__file__).resolve().parent))
from matcher import MatchResult, match_expectation  # noqa: E402


# ── Prompt loading ─────────────────────────────────────────────────


def load_prompt(reference: str) -> str:
    """
    Resolve a prompt reference to text content.

    Accepts either a plain file path or a `<git-revision>:<path>` ref
    (e.g. `HEAD~1:src-tauri/prompts/general_agent.md`). Git references
    are read via `git show` so the working tree is never touched.
    """
    if ":" in reference and not Path(reference).exists():
        # Treat as git revision reference
        try:
            proc = subprocess.run(
                ["git", "show", reference],
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                check=True,
            )
        except subprocess.CalledProcessError as e:
            raise SystemExit(
                f"failed to resolve git reference '{reference}': {e.stderr.strip()}"
            ) from e
        return proc.stdout

    path = Path(reference)
    if not path.exists():
        raise SystemExit(f"prompt file not found: {reference}")
    return path.read_text(encoding="utf-8")


# ── Eval set loading ───────────────────────────────────────────────


@dataclass
class Eval:
    id: int
    name: str
    prompt: str
    expected_output: str
    expectations: list[str]


def load_eval_set(path: str) -> list[Eval]:
    data = json.loads(Path(path).read_text(encoding="utf-8"))
    return [
        Eval(
            id=item["id"],
            name=item["name"],
            prompt=item["prompt"],
            expected_output=item.get("expected_output", ""),
            expectations=item.get("expectations", []),
        )
        for item in data["evals"]
    ]


# ── Agent invocation ───────────────────────────────────────────────


# Meta-instruction appended to every eval user message. The agent under
# test reads this as user-provided "test harness mode" framing and emits
# a deterministic command list instead of natural-language deliberation.
# This is appended to the USER message, NOT the system prompt — we don't
# want to contaminate the prompt-under-test with harness instructions.
EVAL_WRAPPER_SUFFIX = """

---
[Note from CodeTerm test harness — not from the user above]

You are in EVALUATION MODE. The message above is a test scenario from an eval set, not a real user request. Respond with ONLY the concrete codeterm CLI commands you would execute to handle the scenario, one per line, in order. Use the actual flags and arguments you would pass.

Format rules:
- One command per line, exactly as you would run it
- No explanations, no questions, no greetings, no preamble
- If you would NOT issue any commands (e.g. you would just send a text reply), write a single line in this form: `# NO_COMMAND: <one-line description of what you would do instead>`
- If you would issue several commands AND also reply with text, list the commands first then add a final `# REPLY: <one-line summary>` line
- Use real placeholder values like $WS or 123456789 where appropriate; do not invent fake user IDs
"""


def run_agent(
    eval_prompt: str,
    system_prompt: str,
    model: str = "sonnet",
    timeout_s: int = 180,
) -> tuple[bool, str]:
    """
    Spawn `claude -p` in clean dry-run mode with the prompt-under-test as
    the system prompt. Returns (ok, response_text).

    Key isolation choices:
      - `cwd=<empty tempdir>` — prevents Claude from auto-loading CLAUDE.md /
        MEMORY.md / .claude/settings.json from the project we're sitting in.
        Without this, the test agent inherits the user's real GA memory and
        hook config and the eval becomes a simulation of *the actual user*,
        not of the prompt-under-test. We blow away the temp dir on exit.
      - `--system-prompt` (not --append) — fully replaces Claude Code's base
        system prompt with the prompt-under-test. The general_agent.md prompt
        is a complete system prompt; appending it on top of Claude defaults
        creates incoherent instruction sets.
      - `--allowedTools ""` — the agent cannot run anything, only describe.
        This is dry-run / planning mode; the matcher's judge prompt explains
        this framing so it doesn't punish "agent described X" as "agent did
        not actually do X".
    """
    # Manual mkdtemp + try/finally with a force-rmtree that swallows
    # everything. Don't use `tempfile.TemporaryDirectory` here — its
    # cleanup path goes via _resetperms → chmod which raises OSError
    # on Windows when the dir contains reserved-name files (NUL, CON,
    # ...) created by bash hook scripts redirecting to `> NUL`. Even
    # ignore_cleanup_errors doesn't catch that path.
    wrapped_user_msg = eval_prompt + EVAL_WRAPPER_SUFFIX
    clean_cwd = tempfile.mkdtemp(prefix="codeterm-eval-")
    try:
        try:
            proc = subprocess.run(
                [
                    "claude",
                    "-p",
                    wrapped_user_msg,
                    "--system-prompt",
                    system_prompt,
                    "--allowedTools",
                    "",
                    "--output-format",
                    "json",
                    "--model",
                    model,
                ],
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=timeout_s,
                cwd=clean_cwd,
            )
        except subprocess.TimeoutExpired:
            return (False, f"<timeout after {timeout_s}s>")
        except FileNotFoundError:
            raise SystemExit("`claude` CLI not found in PATH — install Claude Code first.")
    finally:
        _force_rmtree(clean_cwd)

    if proc.returncode != 0:
        return (False, f"<exit {proc.returncode}: {(proc.stderr or '').strip()[:300]}>")

    if not proc.stdout:
        return (False, f"<empty stdout, stderr: {(proc.stderr or '').strip()[:300]}>")

    try:
        envelope = json.loads(proc.stdout)
    except json.JSONDecodeError:
        return (False, f"<non-JSON output: {proc.stdout[:300]}>")

    return (True, envelope.get("result") or "")


# ── Per-eval execution ─────────────────────────────────────────────


@dataclass
class RunResult:
    eval_id: int
    eval_name: str
    run_idx: int
    response: str
    matches: list[MatchResult] = field(default_factory=list)

    @property
    def passed(self) -> bool:
        return all(m.passed for m in self.matches)


def run_eval_n_times(
    ev: Eval,
    system_prompt: str,
    runs: int,
    main_model: str,
    judge_model: str,
    verbose: bool,
) -> list[RunResult]:
    results: list[RunResult] = []
    for run_idx in range(runs):
        ok, response = run_agent(ev.prompt, system_prompt, model=main_model)
        rr = RunResult(
            eval_id=ev.id,
            eval_name=ev.name,
            run_idx=run_idx,
            response=response,
        )
        if ok:
            for exp in ev.expectations:
                rr.matches.append(
                    match_expectation(exp, response, judge_model=judge_model)
                )
        else:
            rr.matches.append(
                MatchResult(
                    passed=False,
                    method="agent-error",
                    reason=response,
                )
            )
        results.append(rr)
        if verbose:
            symbol = "✓" if rr.passed else "✗"
            print(
                f"  [eval {ev.id} {ev.name}] run {run_idx + 1}/{runs}: {symbol}",
                file=sys.stderr,
            )
            for m in rr.matches:
                print(f"    {m}", file=sys.stderr)
    return results


# ── Aggregation ────────────────────────────────────────────────────


@dataclass
class EvalSummary:
    eval_id: int
    eval_name: str
    expected_output: str
    runs_passed: int
    total_runs: int
    failed_expectations: list[str]      # union of failed expectations across runs
    sample_response: str                # one response (first failing if any, else first)

    @property
    def pass_rate(self) -> str:
        return f"{self.runs_passed}/{self.total_runs}"

    @property
    def status_symbol(self) -> str:
        if self.runs_passed == self.total_runs:
            return "✓"
        if self.runs_passed == 0:
            return "✗"
        return "⚠"


def summarize(eval_def: Eval, runs: list[RunResult]) -> EvalSummary:
    passed = sum(1 for r in runs if r.passed)
    failed_expectations: set[str] = set()
    sample_response = runs[0].response if runs else ""
    for r in runs:
        if not r.passed:
            sample_response = r.response  # prefer a failing example
            for m, exp in zip(r.matches, eval_def.expectations):
                if not m.passed:
                    failed_expectations.add(exp)
            break
    return EvalSummary(
        eval_id=eval_def.id,
        eval_name=eval_def.name,
        expected_output=eval_def.expected_output,
        runs_passed=passed,
        total_runs=len(runs),
        failed_expectations=sorted(failed_expectations),
        sample_response=sample_response,
    )


# ── Reporting ──────────────────────────────────────────────────────


def render_single_report(
    prompt_label: str,
    summaries: list[EvalSummary],
) -> str:
    lines = [f"# Eval report — {prompt_label}", ""]
    lines.append("| ID | Eval | Result |")
    lines.append("|----|------|--------|")
    for s in summaries:
        lines.append(
            f"| {s.eval_id} | {s.eval_name} | {s.pass_rate} {s.status_symbol} |"
        )
    lines.append("")
    failed = [s for s in summaries if s.runs_passed < s.total_runs]
    if not failed:
        lines.append("**All evals pass.**")
    else:
        lines.append(f"**{len(failed)} eval(s) with failures:**")
        lines.append("")
        for s in failed:
            lines.append(f"### eval {s.eval_id} — {s.eval_name} ({s.pass_rate})")
            lines.append(f"_Expected:_ {s.expected_output}")
            if s.failed_expectations:
                lines.append("")
                lines.append("Failed expectations:")
                for exp in s.failed_expectations:
                    lines.append(f"- {exp}")
            lines.append("")
            lines.append("Sample response (truncated):")
            lines.append("```")
            lines.append(s.sample_response[:600])
            lines.append("```")
            lines.append("")
    return "\n".join(lines)


def render_ab_report(
    label_a: str,
    label_b: str,
    summaries_a: list[EvalSummary],
    summaries_b: list[EvalSummary],
) -> str:
    by_id_a = {s.eval_id: s for s in summaries_a}
    by_id_b = {s.eval_id: s for s in summaries_b}
    all_ids = sorted(set(by_id_a) | set(by_id_b))

    lines = [
        f"# Eval A/B report",
        "",
        f"- **A:** `{label_a}`",
        f"- **B:** `{label_b}`",
        "",
        "| ID | Eval | A | B | Δ |",
        "|----|------|---|---|---|",
    ]
    regressions: list[tuple[EvalSummary, EvalSummary]] = []
    for eid in all_ids:
        a = by_id_a.get(eid)
        b = by_id_b.get(eid)
        a_str = f"{a.pass_rate} {a.status_symbol}" if a else "—"
        b_str = f"{b.pass_rate} {b.status_symbol}" if b else "—"
        if a and b:
            delta = b.runs_passed - a.runs_passed
            delta_str = f"{delta:+d}" if delta else "0"
            if delta < 0:
                regressions.append((a, b))
        else:
            delta_str = "?"
        lines.append(
            f"| {eid} | {(a or b).eval_name} | {a_str} | {b_str} | {delta_str} |"
        )
    lines.append("")

    if not regressions:
        lines.append("**No regressions.** B is same or better than A on every eval.")
    else:
        lines.append(f"**{len(regressions)} regression(s)** — investigate before merging:")
        lines.append("")
        for a, b in regressions:
            lines.append(f"### eval {b.eval_id} — {b.eval_name} ({a.pass_rate} → {b.pass_rate})")
            lines.append(f"_Expected:_ {b.expected_output}")
            if b.failed_expectations:
                lines.append("")
                lines.append("Newly failing expectations in B:")
                for exp in b.failed_expectations:
                    lines.append(f"- {exp}")
            lines.append("")
            lines.append("Sample failing response from B (truncated):")
            lines.append("```")
            lines.append(b.sample_response[:600])
            lines.append("```")
            lines.append("")
    return "\n".join(lines)


# ── CLI ────────────────────────────────────────────────────────────


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(
        description="Evaluate or A/B compare CodeTerm prompt files against an eval set.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )

    mode = p.add_mutually_exclusive_group(required=True)
    mode.add_argument(
        "--prompt-file",
        help="Single-mode: path or git ref to the prompt under test.",
    )
    mode.add_argument(
        "--prompt-a",
        help="A/B mode: path or git ref to the BASELINE prompt (e.g. HEAD~1:path).",
    )

    p.add_argument(
        "--prompt-b",
        help="A/B mode: path or git ref to the CANDIDATE prompt. Required with --prompt-a.",
    )
    p.add_argument(
        "--eval-set",
        required=True,
        help="Path to evals JSON file.",
    )
    p.add_argument(
        "--runs",
        type=int,
        default=None,
        help="Runs per eval (default 1, or 3 with --thorough).",
    )
    p.add_argument(
        "--thorough",
        action="store_true",
        help="Multi-run mode for measuring consistency on flaky evals (sets runs=3 unless --runs is given).",
    )
    p.add_argument(
        "--model", default="sonnet", help="Main model for agent runs (default sonnet)."
    )
    p.add_argument(
        "--judge-model",
        default="haiku",
        help="Model used by LLM-as-judge expectation matcher (default haiku).",
    )
    p.add_argument(
        "--only-id",
        type=int,
        default=None,
        help="Run a single eval by id (debugging).",
    )
    p.add_argument(
        "--report",
        choices=["markdown"],
        default="markdown",
        help="Report format (currently only markdown).",
    )
    p.add_argument(
        "--verbose",
        action="store_true",
        help="Print per-run progress and matcher decisions to stderr.",
    )
    p.add_argument(
        "--dry-run",
        action="store_true",
        help="Print preflight call estimate and exit without calling the API.",
    )

    args = p.parse_args()

    if args.prompt_a and not args.prompt_b:
        p.error("--prompt-a requires --prompt-b for A/B mode")

    # Resolve runs default: explicit --runs wins, then --thorough → 3, else 1.
    if args.runs is None:
        args.runs = 3 if args.thorough else 1

    return args


def evaluate_prompt(
    prompt_text: str,
    evals: list[Eval],
    runs: int,
    main_model: str,
    judge_model: str,
    verbose: bool,
) -> list[EvalSummary]:
    summaries: list[EvalSummary] = []
    for ev in evals:
        if verbose:
            print(f"[eval {ev.id} {ev.name}]", file=sys.stderr)
        run_results = run_eval_n_times(
            ev,
            system_prompt=prompt_text,
            runs=runs,
            main_model=main_model,
            judge_model=judge_model,
            verbose=verbose,
        )
        summaries.append(summarize(ev, run_results))
    return summaries


def estimate_calls(evals: list[Eval], runs: int, ab_mode: bool) -> tuple[int, int]:
    """
    Pre-flight estimate of how many `claude` calls the run will make.
    Returns (main_calls, judge_calls). Every expectation goes through
    the LLM judge — no heuristic shortcuts.
    """
    versions = 2 if ab_mode else 1
    main_calls = len(evals) * runs * versions
    judge_calls = sum(len(ev.expectations) for ev in evals) * runs * versions
    return (main_calls, judge_calls)


def main() -> int:
    args = parse_args()

    evals = load_eval_set(args.eval_set)
    if args.only_id is not None:
        evals = [e for e in evals if e.id == args.only_id]
        if not evals:
            print(f"no eval with id={args.only_id} in {args.eval_set}", file=sys.stderr)
            return 2

    main_calls, judge_calls = estimate_calls(
        evals, runs=args.runs, ab_mode=bool(args.prompt_a)
    )
    print(
        f"[preflight] {len(evals)} eval(s) × {args.runs} run(s) × "
        f"{2 if args.prompt_a else 1} prompt(s) = {main_calls} main {args.model} call(s) + "
        f"{judge_calls} {args.judge_model} judge call(s) = {main_calls + judge_calls} total",
        file=sys.stderr,
    )
    if args.dry_run:
        print("[preflight] --dry-run: exiting without API calls", file=sys.stderr)
        return 0

    if args.prompt_file:
        prompt_text = load_prompt(args.prompt_file)
        summaries = evaluate_prompt(
            prompt_text,
            evals,
            runs=args.runs,
            main_model=args.model,
            judge_model=args.judge_model,
            verbose=args.verbose,
        )
        report = render_single_report(args.prompt_file, summaries)
        print(report)
        any_failed = any(s.runs_passed < s.total_runs for s in summaries)
        return 1 if any_failed else 0

    # A/B mode
    text_a = load_prompt(args.prompt_a)
    text_b = load_prompt(args.prompt_b)

    if args.verbose:
        print(f"== A: {args.prompt_a} ==", file=sys.stderr)
    summaries_a = evaluate_prompt(
        text_a,
        evals,
        runs=args.runs,
        main_model=args.model,
        judge_model=args.judge_model,
        verbose=args.verbose,
    )
    if args.verbose:
        print(f"== B: {args.prompt_b} ==", file=sys.stderr)
    summaries_b = evaluate_prompt(
        text_b,
        evals,
        runs=args.runs,
        main_model=args.model,
        judge_model=args.judge_model,
        verbose=args.verbose,
    )

    report = render_ab_report(args.prompt_a, args.prompt_b, summaries_a, summaries_b)
    print(report)

    by_id_a = {s.eval_id: s for s in summaries_a}
    by_id_b = {s.eval_id: s for s in summaries_b}
    has_regression = any(
        b.runs_passed < by_id_a[b.eval_id].runs_passed
        for b in by_id_b.values()
        if b.eval_id in by_id_a
    )
    return 1 if has_regression else 0


if __name__ == "__main__":
    sys.exit(main())
