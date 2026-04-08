"""
LLM-as-judge expectation matcher for prompt-evaluator.

Every expectation, regardless of shape (technical command, negative
constraint, behavioral rule), is evaluated by a second `claude` call
asking PASS or FAIL. Defaults to Haiku — cheap enough that we don't
need heuristic substring shortcuts, and substring matching has
too many false positives anyway (tokens appearing in unrelated
context, paraphrased commands, etc).

Usage:
    from matcher import match_expectation, MatchResult

    result = match_expectation("Uses workspace create with project name", agent_response)
    if result.passed:
        print(f"PASS: {result.reason}")
"""

from __future__ import annotations

import json
import subprocess
from dataclasses import dataclass


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

Did the agent's command list satisfy the expectation? Reply with EXACTLY one of:
- "PASS: <one short reason>"
- "FAIL: <one short reason>"

Rules:
- Positive expectation naming a command, flag, or argument — the command list must contain that command/flag (PASS) or not (FAIL). Order does not matter.
- Negative expectation ("Does NOT use X" / "Should NOT ...") — the command list must NOT contain X. If X is present anywhere, FAIL. If absent, PASS.
- Behavioral expectation about WHAT the agent does — interpret based on the commands present. E.g. "explores the directory first" passes if `ls`, `tree`, `cat`, `pane exec`, or `send-text "ls ..."` appears before any `workspace create`.
- "Uses send-text --pane general --mesh PEER" requires the response to contain literally `send-text` AND `--pane general` AND `--mesh PEER` (or `--mesh <similar peer name>`).
- If the agent emitted `# NO_COMMAND: ...` and the expectation requires a specific command, FAIL. If the expectation is negative (should NOT do X) and the agent emitted no commands, PASS.
- Ignore minor formatting (extra whitespace, code fences, line numbering).
"""


@dataclass
class MatchResult:
    passed: bool
    method: str          # always "llm-judge" — kept for output formatting compatibility
    reason: str

    def __str__(self) -> str:
        symbol = "✓" if self.passed else "✗"
        return f"{symbol} [{self.method}] {self.reason}"


def match_expectation(
    expectation: str,
    response: str,
    judge_model: str = "haiku",
    timeout_s: int = 60,
) -> MatchResult:
    """
    Run a single judge call. Returns a MatchResult; on any judge failure
    (timeout, parse error, non-zero exit, missing CLI) returns
    `passed=False` with the failure reason — fail closed.
    """
    prompt = JUDGE_PROMPT_TEMPLATE.format(
        expectation=expectation,
        response=response,
    )
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
        verdict = (envelope.get("result") or "").strip()
    except json.JSONDecodeError:
        return MatchResult(False, "llm-judge", f"non-JSON output: {proc.stdout[:200]}")

    if verdict.upper().startswith("PASS"):
        return MatchResult(True, "llm-judge", verdict[5:].lstrip(": ").strip() or "(no reason)")
    if verdict.upper().startswith("FAIL"):
        return MatchResult(False, "llm-judge", verdict[5:].lstrip(": ").strip() or "(no reason)")
    return MatchResult(False, "llm-judge", f"unparseable verdict: {verdict[:200]}")
