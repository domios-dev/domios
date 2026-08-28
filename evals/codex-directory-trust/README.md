# Codex directory trust

This live dev bench proves that newly dispatched Workers receive distinct lane
worktrees and that a Codex tab opened in an untrusted lookalike directory stays
at its directory-trust prompt instead of becoming ready.

## Run

Run against an exact-head dev daemon on port 7686:

```bash
bash evals/codex-directory-trust/run.sh
```

## Pass

The script ends with two `PASS:` lines: one reports two distinct ready Worker
lanes, and the other reports that the foreign tab retained the trust prompt.

<!-- revision: 1 -->
