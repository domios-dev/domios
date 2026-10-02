# Provider prompt matrix

Source phase only. No live matrix or red-green demonstration ran in this lane.

The 87 cells below comprise 86 NOT RUN and one owner-accepted N/A. No transcript is claimed. The release gate remains closed.

Jev off is the heuristic arm. Jev on is the primary adapter arm with any actual fallback recorded. Deciding paths remain unobserved until journal-backed execution. Auditor disagreement points are pending its covered-point list.

| Provider | Case | Variant | Jev off result / path | Jev on result / path | Transcript |
|---|---|---|---|---|---|
| claude | trust-hidden | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| claude | trust-cron | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| claude | trust-visible | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| claude | trust-lane | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| claude | update | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| claude | login | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| claude | permission | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| claude | bad-model | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| claude | crash | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| codex | trust-hidden | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| codex | trust-cron | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| codex | trust-visible | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| codex | trust-lane | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| codex | update | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| codex | login | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| codex | permission | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| codex | bad-model | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| codex | crash | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| opencode | trust-hidden | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| opencode | trust-cron | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| opencode | trust-visible | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| opencode | trust-lane | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| opencode | update | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| opencode | login | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| opencode | permission | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| opencode | bad-model | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| opencode | crash | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| cursor | trust-hidden | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| cursor | trust-cron | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| cursor | trust-visible | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| cursor | trust-lane | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| cursor | update | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| cursor | login | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| cursor | permission | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| cursor | bad-model | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| cursor | crash | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| codex | update-coordinator | default | NOT RUN / unobserved | NOT RUN / unobserved | none |
| codex | update-coordinator | busy | NOT RUN / unobserved | NOT RUN / unobserved | none |
| codex | update-coordinator | fail | NOT RUN / unobserved | NOT RUN / unobserved | none |
| host | sentinel | git | NOT RUN / unobserved | NOT RUN / unobserved | none |
| host | sentinel | fake-ci | NOT RUN / unobserved | NOT RUN / unobserved | none |
| host | sentinel | regex-no-jev | NOT RUN / unobserved | NOT RUN / unobserved | none |
| host | sentinel | budget-exhausted | N/A: regex never spends model budget; owner accepted (Brahms, 2026-10-02) | NOT RUN / unobserved | none |
| host | sentinel | activity-feed | No off-arm cell: feed projection regression | NOT RUN / expected RED: persisted Sentinel rows are currently excluded | none |

Counts: PASS=0; FAIL=0; NOT RUN=86; N/A=1. Auditor disagreement cells: pending point list, additional to these 87.

Owner decision: budget-exhausted with Jev off is not applicable because regex mode never spends model budget. The Jev-on explicit cap plus a disconnected regex companion covers exhaustion. Source binding does not claim an executed outcome.

Source-expected red: Sentinel verdicts must appear in the activity feed. The feed cell and focused Rust regression are authored to fail until the scheduled repair lands. No live FAIL has been observed here. Lookout CLI/outcome and Lens cap contracts are bound; Auditor paths and Courier remain PENDING.

Executed evidence: ACK confirmed; source/CLI-help discovery only. No provider was launched, no prompt provoked, and no dev or project gate executed.

Red-green: deferred to Orchestrator canonical QA. Source deltas and exact gates are in README.md and GATES.md. contracts.json records unbound dependency contracts.
