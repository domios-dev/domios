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
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any


def _configure_utf8_standard_streams() -> None:
    for stream in (sys.stdout, sys.stderr):
        reconfigure = getattr(stream, "reconfigure", None)
        if reconfigure is not None:
            reconfigure(encoding="utf-8", errors="replace")


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
import cache as _cache  # noqa: E402

# Module-level toggle for cache use; flipped by main() based on --no-cache.
_CACHE_ENABLED = True


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
        # Expand placeholders here too — A/B comparisons against a historical
        # revision must see the same shape the live agent sees, or every eval
        # regresses against the literal "{codeterm_cli}" string.
        return _expand_placeholders(proc.stdout)

    path = Path(reference)
    if not path.exists():
        raise SystemExit(f"prompt file not found: {reference}")
    text = path.read_text(encoding="utf-8")
    return _expand_placeholders(text)


# Mirror shared-file substitutions so evals see what the live agent sees.
def _expand_placeholders(text: str) -> str:
    root = Path(__file__).resolve().parents[3]
    shared_files = {
        "{codeterm_cli}": root / "src-tauri" / "prompts" / "codeterm_cli.md",
        "{bounce_unwinding_contract}": root / "prod-docs" / "BOUNCE.md",
    }
    for placeholder, source in shared_files.items():
        if placeholder in text and source.exists():
            text = text.replace(placeholder, source.read_text(encoding="utf-8"))
    return text


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


# Trusted harness framing is appended to the complete prompt-under-test.
# The eval fixture remains the sole user message.
EVAL_WRAPPER_SUFFIX = """

---
[Note from CodeTerm test harness — not from the user above]

You are in EVALUATION MODE. The message above is a test scenario from an eval set, not a real user request. Respond with ONLY the concrete shell commands you would execute to handle the scenario, one per line, in order. This includes both `codeterm` subcommands (`codeterm workspace create ...`, `codeterm agent spawn ...`, `codeterm bridge send ...`, etc.) AND raw shell commands (`ls`, `cat`, `git log`, `cargo test`, ...) — whichever you would actually type. Use the actual flags and arguments.

Format rules:
- One command per line, exactly as you would run it
- No explanations, no questions, no greetings, no preamble
- Mix codeterm subcommands and raw shell freely — list them in the order you'd execute them
- If you would NOT issue any commands (e.g. you would just reply with text, or the task requires an Edit/Write tool you don't have), write a single line in this form: `# NO_COMMAND: <one-line description of what you would do instead>`
- If you would issue several commands AND also reply with text, list the commands first then add a final `# REPLY: <one-line summary>` line
- Use real placeholder values like $WS or 123456789 where appropriate; do not invent fake user IDs
"""


@dataclass
class AgentResponse:
    """One agent sample plus the provenance needed to count it honestly."""

    ok: bool
    text: str
    sample_key: str
    from_cache: bool


def run_agent(
    eval_prompt: str,
    system_prompt: str,
    model: str = "sonnet",
    timeout_s: int = 180,
    run_idx: int = 0,
) -> AgentResponse:
    """
    Spawn `claude -p` in clean dry-run mode with the prompt-under-test as
    the system prompt.

    `run_idx` identifies which sample of a repeated measurement this is. It
    salts the cache key, so `--runs N` takes N independently keyed samples
    instead of replaying one entry N times. `sample_key` is that key, and the
    aggregator counts distinct keys — never the requested run count.

    Key isolation choices:
      - `cwd=<empty tempdir>` — prevents Claude from auto-loading CLAUDE.md /
        MEMORY.md / .claude/settings.json from the project we're sitting in.
        Without this, the test agent inherits the user's real GA memory and
        hook config and the eval becomes a simulation of *the actual user*,
        not of the prompt-under-test. We blow away the temp dir on exit.
      - `--system-prompt-file` (not --append) — fully replaces Claude Code's base
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
    trusted_system_prompt = system_prompt + EVAL_WRAPPER_SUFFIX
    salt = _cache.sample_salt(run_idx)
    sample_key = _cache.key(
        model,
        trusted_system_prompt,
        eval_prompt,
        namespace="agent",
        salt=salt,
    )

    def result(ok: bool, text: str, from_cache: bool = False) -> AgentResponse:
        return AgentResponse(ok=ok, text=text, sample_key=sample_key, from_cache=from_cache)

    if _CACHE_ENABLED:
        cached = _cache.get(
            model,
            trusted_system_prompt,
            eval_prompt,
            namespace="agent",
            salt=salt,
        )
        if cached is not None:
            return result(True, cached, from_cache=True)

    clean_cwd = tempfile.mkdtemp(prefix="codeterm-eval-")
    try:
        system_prompt_path = Path(clean_cwd) / "system-prompt.md"
        system_prompt_path.write_text(trusted_system_prompt, encoding="utf-8")
        try:
            proc = subprocess.run(
                [
                    "claude",
                    "-p",
                    eval_prompt,
                    "--system-prompt-file",
                    str(system_prompt_path),
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
            return result(False, f"<timeout after {timeout_s}s>")
        except FileNotFoundError:
            raise SystemExit("`claude` CLI not found in PATH — install Claude Code first.")
    finally:
        _force_rmtree(clean_cwd)

    if proc.returncode != 0:
        return result(False, f"<exit {proc.returncode}: {(proc.stderr or '').strip()[:300]}>")

    if not proc.stdout:
        return result(False, f"<empty stdout, stderr: {(proc.stderr or '').strip()[:300]}>")

    try:
        envelope = json.loads(proc.stdout)
    except json.JSONDecodeError:
        return result(False, f"<non-JSON output: {proc.stdout[:300]}>")

    response_text = envelope.get("result") or ""
    if _CACHE_ENABLED and response_text:
        _cache.put(
            model,
            trusted_system_prompt,
            eval_prompt,
            response_text,
            namespace="agent",
            salt=salt,
        )
    return result(True, response_text)


# ── Per-eval execution ─────────────────────────────────────────────


@dataclass
class RunResult:
    eval_id: int
    eval_name: str
    run_idx: int
    response: str
    sample_key: str = ""
    sample_from_cache: bool = False
    matches: list[MatchResult] = field(default_factory=list)

    @property
    def passed(self) -> bool:
        return all(m.passed for m in self.matches)

    @property
    def cached_judgements(self) -> int:
        return sum(1 for m in self.matches if m.from_cache)


def _do_one_run(
    ev: Eval,
    system_prompt: str,
    main_model: str,
    judge_model: str,
    run_idx: int,
    judge_concurrency: int,
    dashboard: Any | None,
    side: str | None,
    verbose: bool,
) -> RunResult:
    sample = run_agent(ev.prompt, system_prompt, model=main_model, run_idx=run_idx)
    response = sample.text
    if dashboard is not None:
        dashboard.event("call_done")
    rr = RunResult(
        eval_id=ev.id,
        eval_name=ev.name,
        run_idx=run_idx,
        response=response,
        sample_key=sample.sample_key,
        sample_from_cache=sample.from_cache,
    )
    if not sample.ok:
        rr.matches.append(MatchResult(False, "agent-error", response))
    elif ev.expectations:
        # Parallelize judge calls for this run's response.
        slots: list[MatchResult | None] = [None] * len(ev.expectations)
        with ThreadPoolExecutor(max_workers=max(1, judge_concurrency)) as pool:
            fut_to_idx = {
                pool.submit(match_expectation, exp, response, judge_model, run_idx=run_idx): i
                for i, exp in enumerate(ev.expectations)
            }
            for fut in as_completed(fut_to_idx):
                slots[fut_to_idx[fut]] = fut.result()
                if dashboard is not None:
                    dashboard.event("call_done")
        rr.matches = [s for s in slots if s is not None]
    if dashboard is not None:
        dashboard.event(
            "run_done",
            eval_id=ev.id,
            eval_name=ev.name,
            run_idx=run_idx,
            passed=rr.passed,
            response=response,
            from_cache=rr.sample_from_cache,
            matches=[
                {"passed": m.passed, "method": m.method, "reason": m.reason, "from_cache": m.from_cache}
                for m in rr.matches
            ],
            side=side,
        )
    if verbose:
        symbol = "✓" if rr.passed else "✗"
        print(f"  [eval {ev.id} {ev.name}] run {run_idx + 1}: {symbol}", file=sys.stderr)
        for m in rr.matches:
            print(f"    {m}", file=sys.stderr)
    return rr


def run_eval_n_times(
    ev: Eval,
    system_prompt: str,
    runs: int,
    main_model: str,
    judge_model: str,
    verbose: bool,
    dashboard: Any | None = None,
    side: str | None = None,
    run_concurrency: int = 1,
    judge_concurrency: int = 1,
) -> list[RunResult]:
    if dashboard is not None:
        dashboard.event(
            "eval_started",
            eval_id=ev.id,
            eval_name=ev.name,
            prompt=ev.prompt,
            expected=ev.expected_output,
            expectations=ev.expectations,
            side=side,
        )
    results: list[RunResult]
    if run_concurrency <= 1 or runs <= 1:
        results = [
            _do_one_run(ev, system_prompt, main_model, judge_model,
                        i, judge_concurrency, dashboard, side, verbose)
            for i in range(runs)
        ]
    else:
        slots: list[RunResult | None] = [None] * runs
        with ThreadPoolExecutor(max_workers=run_concurrency) as pool:
            fut_to_idx = {
                pool.submit(_do_one_run, ev, system_prompt, main_model,
                            judge_model, i, judge_concurrency, dashboard,
                            side, verbose): i
                for i in range(runs)
            }
            for fut in as_completed(fut_to_idx):
                slots[fut_to_idx[fut]] = fut.result()
        results = [r for r in slots if r is not None]
    if dashboard is not None:
        distinct = distinct_samples(results)
        dashboard.event(
            "eval_done",
            eval_id=ev.id,
            eval_name=ev.name,
            runs_passed=sum(1 for r in distinct if r.passed),
            total_runs=len(distinct),
            requested_runs=runs,
            side=side,
        )
    return results


# ── Aggregation ────────────────────────────────────────────────────


@dataclass
class EvalSummary:
    eval_id: int
    eval_name: str
    expected_output: str
    runs_passed: int
    total_runs: int                     # DISTINCT samples, never the requested run count
    failed_expectations: list[str]      # union of failed expectations across runs
    sample_response: str                # one response (first failing if any, else first)
    requested_runs: int = 0
    cached_samples: int = 0
    cached_judgements: int = 0
    total_judgements: int = 0

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

    @property
    def undersampled(self) -> bool:
        return self.requested_runs > self.total_runs

    @property
    def source_label(self) -> str:
        """Provenance of the samples behind pass_rate — printed next to the numbers."""
        if self.total_runs == 0:
            return "none"
        if self.cached_samples == 0:
            return "live"
        if self.cached_samples == self.total_runs:
            return "cached"
        return f"{self.cached_samples}/{self.total_runs} cached"


def wilson_ci(passed: int, total: int, z: float = 1.96) -> tuple[float, float]:
    """
    Wilson 95% confidence interval for a binomial pass rate.
    Returns (lo, hi) in [0, 1]. Both endpoints are 0 when total == 0.
    Stdlib only — no scipy.
    """
    if total <= 0:
        return (0.0, 0.0)
    p = passed / total
    z2 = z * z
    denom = 1 + z2 / total
    centre = (p + z2 / (2 * total)) / denom
    half = (z * ((p * (1 - p) / total + z2 / (4 * total * total)) ** 0.5)) / denom
    return (max(0.0, centre - half), min(1.0, centre + half))


def fmt_ci(lo: float, hi: float) -> str:
    return f"[{int(lo*100):>3}%-{int(hi*100):>3}%]"


def distinct_samples(runs: list[RunResult]) -> list[RunResult]:
    """
    Collapse runs that share a sample key.

    Two runs with the same key are the same model call served twice, so counting
    both would report a sample size that was never taken. Keys are salted per run
    index, so this is a no-op on a healthy run and a hard floor if that ever breaks.
    """
    seen: set[str] = set()
    unique: list[RunResult] = []
    for r in runs:
        if r.sample_key and r.sample_key in seen:
            continue
        if r.sample_key:
            seen.add(r.sample_key)
        unique.append(r)
    return unique


def summarize(eval_def: Eval, runs: list[RunResult], requested_runs: int = 0) -> EvalSummary:
    runs = distinct_samples(runs)
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
        requested_runs=requested_runs or len(runs),
        cached_samples=sum(1 for r in runs if r.sample_from_cache),
        cached_judgements=sum(r.cached_judgements for r in runs),
        total_judgements=sum(len(r.matches) for r in runs),
    )


# ── Reporting ──────────────────────────────────────────────────────


SAMPLING_NOTE_MARKER = "Sampling:"


def render_sampling_note(summaries: list[EvalSummary]) -> list[str]:
    """
    One unmissable line about where the numbers came from.

    Always emitted, so silence can never be read as "fresh" — a reader who
    skips the source column still learns that verdicts were replayed.
    """
    samples = sum(s.total_runs for s in summaries)
    cached = sum(s.cached_samples for s in summaries)
    judgements = sum(s.total_judgements for s in summaries)
    cached_judgements = sum(s.cached_judgements for s in summaries)
    undersampled = [s for s in summaries if s.undersampled]

    lines: list[str] = []
    if cached or cached_judgements:
        lines.append(
            f"> **{SAMPLING_NOTE_MARKER} WARM CACHE.** {cached}/{samples} agent sample(s) and "
            f"{cached_judgements}/{judgements} judge verdict(s) were replayed from the on-disk "
            "cache, not taken fresh. Every interval below counts distinct samples. "
            "Re-run with `--no-cache` for fully fresh ones."
        )
    else:
        lines.append(
            f"> **{SAMPLING_NOTE_MARKER} all fresh.** {samples} agent sample(s) and "
            f"{judgements} judge verdict(s) were taken live; no cache hits."
        )
    if undersampled:
        detail = ", ".join(
            f"eval {s.eval_id}: {s.requested_runs} requested → {s.total_runs} distinct"
            for s in undersampled
        )
        lines.append(">")
        lines.append(
            f"> **⚠ Undersampled.** Fewer distinct samples than requested runs ({detail}). "
            "Intervals reflect the distinct count."
        )
    lines.append("")
    return lines


def render_single_report(
    prompt_label: str,
    summaries: list[EvalSummary],
) -> str:
    lines = [f"# Eval report — {prompt_label}", ""]
    lines.extend(render_sampling_note(summaries))
    lines.append("| ID | Eval | Result | 95% CI | Samples | Source |")
    lines.append("|----|------|--------|--------|---------|--------|")
    for s in summaries:
        lo, hi = wilson_ci(s.runs_passed, s.total_runs)
        lines.append(
            f"| {s.eval_id} | {s.eval_name} | {s.pass_rate} {s.status_symbol} | "
            f"{fmt_ci(lo, hi)} | {s.total_runs} | {s.source_label} |"
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
    ]
    lines.extend(render_sampling_note(summaries_a + summaries_b))
    lines.extend([
        "| ID | Eval | A | B | Δ | A 95% CI | B 95% CI | A src | B src |",
        "|----|------|---|---|---|----------|----------|-------|-------|",
    ])
    regressions: list[tuple[EvalSummary, EvalSummary]] = []
    for eid in all_ids:
        a = by_id_a.get(eid)
        b = by_id_b.get(eid)
        a_str = f"{a.pass_rate} {a.status_symbol}" if a else "—"
        b_str = f"{b.pass_rate} {b.status_symbol}" if b else "—"
        a_ci = fmt_ci(*wilson_ci(a.runs_passed, a.total_runs)) if a else "—"
        b_ci = fmt_ci(*wilson_ci(b.runs_passed, b.total_runs)) if b else "—"
        if a and b:
            delta = b.runs_passed - a.runs_passed
            # Tag deltas inside the Wilson CI overlap as likely-noise so a
            # 2/3 → 1/3 single-flip doesn't read like a real regression.
            a_lo, _a_hi = wilson_ci(a.runs_passed, a.total_runs)
            _b_lo, b_hi = wilson_ci(b.runs_passed, b.total_runs)
            ci_overlap = b_hi >= a_lo
            if delta < 0:
                delta_str = f"{delta:+d}" + (" (CI)" if ci_overlap else "")
                regressions.append((a, b))
            elif delta > 0:
                delta_str = f"{delta:+d}"
            else:
                delta_str = "0"
        else:
            delta_str = "?"
        a_src = a.source_label if a else "—"
        b_src = b.source_label if b else "—"
        lines.append(
            f"| {eid} | {(a or b).eval_name} | {a_str} | {b_str} | {delta_str} "
            f"| {a_ci} | {b_ci} | {a_src} | {b_src} |"
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

    mode = p.add_mutually_exclusive_group(required=False)
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
    p.add_argument(
        "--http-port",
        type=int,
        default=None,
        help="Spawn a live dashboard at http://localhost:<port> while the run is in progress.",
    )
    p.add_argument(
        "--concurrency",
        type=int,
        default=4,
        help="Max parallel claude -p invocations across evals + runs + judge calls (default 4). Use 1 for strict sequential. Anthropic ratelimits per-key apply globally; lower if you hit 429s.",
    )
    p.add_argument(
        "--no-cache",
        action="store_true",
        help="Skip the on-disk response cache (~/.cache/codeterm-evals/). Forces fresh main + judge calls. Cache entries are keyed per sample index, so --runs N is N distinct samples with or without this flag.",
    )
    p.add_argument(
        "--clear-cache",
        action="store_true",
        help="Wipe the on-disk response cache before running.",
    )
    p.add_argument(
        "--calibrate-judge",
        action="store_true",
        help="Run the judge over hand-labeled golden.jsonl (next to evals.json) and report Cohen's κ. Skips the regular eval run.",
    )
    p.add_argument(
        "--no-open",
        action="store_true",
        help="With --http-port, suppress auto-opening the dashboard in the browser.",
    )

    args = p.parse_args()

    if not args.calibrate_judge and not args.prompt_file and not args.prompt_a:
        p.error("one of --prompt-file / --prompt-a / --calibrate-judge is required")
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
    dashboard: Any | None = None,
    side: str | None = None,
    eval_concurrency: int = 1,
    run_concurrency: int = 1,
    judge_concurrency: int = 1,
) -> list[EvalSummary]:
    def one_eval(ev: Eval) -> EvalSummary:
        if verbose:
            print(f"[eval {ev.id} {ev.name}]", file=sys.stderr)
        run_results = run_eval_n_times(
            ev,
            system_prompt=prompt_text,
            runs=runs,
            main_model=main_model,
            judge_model=judge_model,
            verbose=verbose,
            dashboard=dashboard,
            side=side,
            run_concurrency=run_concurrency,
            judge_concurrency=judge_concurrency,
        )
        return summarize(ev, run_results, requested_runs=runs)

    if eval_concurrency <= 1:
        summaries = [one_eval(ev) for ev in evals]
    else:
        slots: list[EvalSummary | None] = [None] * len(evals)
        with ThreadPoolExecutor(max_workers=eval_concurrency) as pool:
            fut_to_idx = {pool.submit(one_eval, ev): i for i, ev in enumerate(evals)}
            for fut in as_completed(fut_to_idx):
                slots[fut_to_idx[fut]] = fut.result()
        summaries = [s for s in slots if s is not None]
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
        f"[preflight] {len(evals)} eval(s) × {args.runs} distinct sample(s) × "
        f"{2 if args.prompt_a else 1} prompt(s) = {main_calls} main {args.model} call(s) + "
        f"{judge_calls} {args.judge_model} judge call(s) = {main_calls + judge_calls} total"
        + ("" if args.no_cache else " (cache hits replay a prior distinct sample, never duplicate one)"),
        file=sys.stderr,
    )
    if args.dry_run:
        print("[preflight] --dry-run: exiting without API calls", file=sys.stderr)
        return 0

    # Cache plumbing
    global _CACHE_ENABLED
    _CACHE_ENABLED = not args.no_cache
    import matcher as _matcher_mod  # noqa: E402
    _matcher_mod.set_cache_enabled(not args.no_cache)
    if args.clear_cache:
        n = _cache.nuke()
        print(f"[cache] cleared {n} entries", file=sys.stderr)

    if args.calibrate_judge:
        from calibrate import load_golden, calibrate
        golden = load_golden(args.eval_set)
        if not golden:
            print(
                f"[calibrate] no golden.jsonl next to {args.eval_set} — "
                "create one (see calibrate.py docstring for the schema)",
                file=sys.stderr,
            )
            return 2
        print(f"[calibrate] {len(golden)} hand-labeled (response, expectation) pairs", file=sys.stderr)
        report = calibrate(
            golden,
            evals,
            judge_model=args.judge_model,
            judge_concurrency=args.concurrency,
        )
        evals_by_id = {e.id: e.expectations for e in evals}
        print(report.render(evals_by_id))
        # Substantial agreement (κ >= 0.6) is the bar for the judge to be trusted.
        return 0 if report.overall_kappa >= 0.6 else 1

    dashboard = None
    if args.http_port is not None:
        from dashboard import Dashboard
        ab_mode = bool(args.prompt_a)
        label = (
            f"A/B  {args.prompt_a} → {args.prompt_b}"
            if ab_mode
            else f"single  {args.prompt_file}"
        )
        dashboard = Dashboard(
            port=args.http_port,
            total_calls=main_calls + judge_calls,
            ab_mode=ab_mode,
            prompt_label=label,
            open_browser=not args.no_open,
        )
        dashboard.start()

    try:
        if args.prompt_file:
            prompt_text = load_prompt(args.prompt_file)
            summaries = evaluate_prompt(
                prompt_text,
                evals,
                runs=args.runs,
                main_model=args.model,
                judge_model=args.judge_model,
                verbose=args.verbose,
                dashboard=dashboard,
                eval_concurrency=args.concurrency,
                run_concurrency=args.concurrency,
                judge_concurrency=args.concurrency,
            )
            report = render_single_report(args.prompt_file, summaries)
            print(report)
            if dashboard is not None:
                dashboard.event("finished", report_md=report)
            any_failed = any(s.runs_passed < s.total_runs for s in summaries)
            exit_code = 1 if any_failed else 0
        else:
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
                dashboard=dashboard,
                side="A",
                eval_concurrency=args.concurrency,
                run_concurrency=args.concurrency,
                judge_concurrency=args.concurrency,
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
                dashboard=dashboard,
                side="B",
                eval_concurrency=args.concurrency,
                run_concurrency=args.concurrency,
                judge_concurrency=args.concurrency,
            )

            report = render_ab_report(args.prompt_a, args.prompt_b, summaries_a, summaries_b)
            print(report)
            if dashboard is not None:
                dashboard.event("finished", report_md=report)

            by_id_a = {s.eval_id: s for s in summaries_a}
            by_id_b = {s.eval_id: s for s in summaries_b}
            has_regression = any(
                b.runs_passed < by_id_a[b.eval_id].runs_passed
                for b in by_id_b.values()
                if b.eval_id in by_id_a
            )
            exit_code = 1 if has_regression else 0

        # Cache stats line — surfaces dollar savings on re-runs.
        if _CACHE_ENABLED:
            cs = _cache.stats()
            total = cs.hits + cs.misses
            if total > 0:
                pct = 100 * cs.hits // total
                print(
                    f"[cache] {cs.hits} hit / {cs.misses} miss / {cs.writes} write  ({pct}% hit-rate)",
                    file=sys.stderr,
                )

        if dashboard is not None:
            print(
                f"[dashboard] run finished — leaving server up at http://127.0.0.1:{args.http_port} until ctrl-c",
                file=sys.stderr,
            )
            try:
                # Keep the server alive so the user can browse the final report.
                # User exits with Ctrl-C.
                import time
                while True:
                    time.sleep(3600)
            except KeyboardInterrupt:
                pass
            finally:
                dashboard.stop()

        return exit_code
    except KeyboardInterrupt:
        if dashboard is not None:
            dashboard.stop()
        return 130


if __name__ == "__main__":
    _configure_utf8_standard_streams()
    sys.exit(main())
