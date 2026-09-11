"""
LLM-as-judge expectation matcher for prompt-evaluator.

Every expectation, regardless of shape (technical command, negative
constraint, behavioral rule), is evaluated by a second `claude` call
asking PASS or FAIL. Defaults to Haiku — cheap enough that we don't
need heuristic substring shortcuts, and substring matching has
too many false positives anyway (tokens appearing in unrelated
context, paraphrased commands, etc).

The judge is given a chain-of-thought scratchpad before its verdict
(G-Eval / Anthropic eval-cookbook recommendation: thinking-before-
scoring raises agreement with human raters meaningfully). The
reasoning is parsed but discarded for pass/fail decisions; the final
VERDICT line is what matters.

Usage:
    from matcher import match_expectation, MatchResult

    result = match_expectation("Uses workspace create with project name", agent_response)
    if result.passed:
        print(f"PASS: {result.reason}")
"""

from __future__ import annotations

import json
import re
import subprocess
import tempfile
from dataclasses import dataclass

try:
    from . import cache as _cache  # type: ignore
except ImportError:
    import cache as _cache  # noqa: E402

sample_salt = _cache.sample_salt

# Module-level toggle for cache use; flipped by runner based on --no-cache.
_CACHE_ENABLED = True


def set_cache_enabled(enabled: bool) -> None:
    global _CACHE_ENABLED
    _CACHE_ENABLED = enabled


JUDGE_PROMPT_TEMPLATE = """\
You are evaluating an AI agent's command-list response against a single expectation.

CONTEXT: The agent under test was given a user scenario AND a test-harness
instruction telling it to respond ONLY with the concrete codeterm CLI commands
it would execute, one per line. So the agent's response below should look
like a script — one command per line, optionally with `# NO_COMMAND: ...` or
`# REPLY: ...` annotation lines. Your job is to check whether the command
list satisfies the expectation.

EXPECTATION:
{expectation}

AGENT RESPONSE (command list):
{response}

Think step by step inside <reasoning>...</reasoning> tags before deciding:
1. Classify the expectation: positive (requires X), negative (forbids X), or behavioral (an inferred action).
2. Scan the response for the relevant commands / flags / patterns.
3. Decide PASS / FAIL based on the rules below.

Rules:
- Positive expectation naming a command, flag, or argument — the command list must contain that command/flag (PASS) or not (FAIL). Order does not matter.
- Negative expectation ("Does NOT use X" / "Should NOT ...") — the command list must NOT contain X. If X is present anywhere, FAIL. If absent, PASS.
- Behavioral expectation about WHAT the agent does — interpret based on the commands present. E.g. "explores the directory first" passes if `ls`, `tree`, `cat`, `tab exec`, or `send-text "ls ..."` appears before any `workspace create`.
- "Uses send-text --tab general --mesh PEER" requires the response to contain literally `send-text` AND `--tab general` AND `--mesh PEER` (or `--mesh <similar peer name>`).
- If the agent emitted `# NO_COMMAND: ...` and the expectation requires a specific command, FAIL. If the expectation is negative (should NOT do X) and the agent emitted no commands, PASS.
- Ignore minor formatting (extra whitespace, code fences, line numbering).

After </reasoning>, output exactly ONE final line in this exact format:
VERDICT: PASS — <one short reason>
or
VERDICT: FAIL — <one short reason>

Do NOT output anything after the VERDICT line.
"""


_VERDICT_RE = re.compile(r"VERDICT:\s*(PASS|FAIL)\b\s*[:—–-]?\s*(.*?)\s*$", re.IGNORECASE | re.MULTILINE)


def sandbox_env() -> dict:
    """Environment for a nested `claude` call.

    A nested call must not present the caller's CodeTerm identity: the
    provider hooks are machine-global, so an inherited CODETERM_TAB_ID makes
    the daemon attribute the nested session to the calling tab.
    """
    import os

    return {k: v for k, v in os.environ.items() if not k.startswith("CODETERM_")}


def _parse_verdict(text: str) -> tuple[bool | None, str]:
    """
    Pull the final VERDICT line out of a chain-of-thought response.
    Returns (passed, reason). passed is None if the verdict couldn't be parsed.
    """
    matches = list(_VERDICT_RE.finditer(text))
    if matches:
        m = matches[-1]
        verdict = m.group(1).upper()
        reason = (m.group(2) or "").strip() or "(no reason)"
        return (verdict == "PASS", reason)
    # Fallback for old-style "PASS: ..." / "FAIL: ..." replies (judge ignored CoT format).
    stripped = text.strip()
    upper = stripped.upper()
    if upper.startswith("PASS"):
        return (True, stripped[5:].lstrip(": —-").strip() or "(no reason)")
    if upper.startswith("FAIL"):
        return (False, stripped[5:].lstrip(": —-").strip() or "(no reason)")
    return (None, "")


@dataclass
class MatchResult:
    passed: bool
    method: str          # always "llm-judge" — kept for output formatting compatibility
    reason: str
    from_cache: bool = False

    def __str__(self) -> str:
        symbol = "✓" if self.passed else "✗"
        return f"{symbol} [{self.method}] {self.reason}"


def match_expectation(
    expectation: str,
    response: str,
    judge_model: str = "haiku",
    timeout_s: int = 60,
    run_idx: int = 0,
) -> MatchResult:
    """
    Run a single judge call. Returns a MatchResult; on any judge failure
    (timeout, parse error, non-zero exit, missing CLI) returns
    `passed=False` with the failure reason — fail closed.

    `run_idx` salts the cache key so repeated samples get independent
    verdicts even when two samples produced byte-identical responses.
    """
    prompt = JUDGE_PROMPT_TEMPLATE.format(
        expectation=expectation,
        response=response,
    )
    salt = sample_salt(run_idx)
    # judge has no system prompt — pass empty string as the system slot for cache key.
    if _CACHE_ENABLED:
        cached = _cache.get(judge_model, "", prompt, namespace="judge", salt=salt)
        if cached is not None:
            passed, reason = _parse_verdict(cached)
            if passed is not None:
                return MatchResult(passed, "llm-judge", reason, from_cache=True)
            # cached entry was malformed — fall through and re-run.
    try:
        proc = subprocess.run(
            [
                "claude",
                "-p",
                prompt,
                "--model",
                judge_model,
                "--allowedTools",
                "",
                "--output-format",
                "json",
            ],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout_s,
            env=sandbox_env(),
            cwd=tempfile.mkdtemp(prefix="codeterm-judge-"),
        )
    except subprocess.TimeoutExpired:
        return MatchResult(False, "llm-judge", f"judge timeout after {timeout_s}s")
    except FileNotFoundError:
        return MatchResult(False, "llm-judge", "judge error: `claude` CLI not found in PATH")

    if proc.returncode != 0:
        return MatchResult(
            False,
            "llm-judge",
            f"judge exit {proc.returncode}: {(proc.stderr or '').strip()[:200]}",
        )

    if not proc.stdout:
        return MatchResult(False, "llm-judge", "empty stdout from judge")

    try:
        envelope = json.loads(proc.stdout)
        verdict_text = (envelope.get("result") or "").strip()
    except json.JSONDecodeError:
        return MatchResult(False, "llm-judge", f"non-JSON output: {proc.stdout[:200]}")

    passed, reason = _parse_verdict(verdict_text)
    if passed is None:
        return MatchResult(False, "llm-judge", f"unparseable verdict: {verdict_text[:200]}")
    if _CACHE_ENABLED:
        _cache.put(judge_model, "", prompt, verdict_text, namespace="judge", salt=salt)
    return MatchResult(passed, "llm-judge", reason, from_cache=False)
