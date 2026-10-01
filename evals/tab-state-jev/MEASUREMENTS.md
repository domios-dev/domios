# Tab-state Jev measurements

Captured and measured 2026-10-01 on the named `tabstate` dev instance. This is
an exploratory result: `screens.jsonl` has 60 human-labelled frames, all with
capability-declared `pty` transport. It is missing 33 required provider/label
groups, and the instance has no LM Studio chat-backend provider. Every reported
gate is therefore ineligible; these numbers do not authorize runtime actions.

The default evaluator rejected the incomplete matrix before making a Jev call.
Exploratory mode then used 60 calls per run (180 total), with 60 successful
responses each. No run exceeded the 300-call limit.

## Per-label precision / recall

`—` means the model made no prediction for that label, so precision is
undefined. Support is the number of labelled frames.

| Label | Support | Screen P / R | Context, current P / R | Context, first-run P / R |
| --- | ---: | ---: | ---: | ---: |
| ready_idle | 16 | 0.696 / 1.000 | 0.800 / 1.000 | 0.941 / 1.000 |
| working_normally | 10 | 1.000 / 0.800 | 1.000 / 0.900 | 1.000 / 0.900 |
| provider_update_prompt | 1 | 1.000 / 1.000 | 1.000 / 1.000 | 1.000 / 1.000 |
| trust_or_permission_prompt | 9 | 1.000 / 1.000 | 1.000 / 1.000 | 1.000 / 1.000 |
| login_or_auth_required | 9 | 1.000 / 0.444 | 1.000 / 0.444 | 1.000 / 1.000 |
| model_unavailable_or_quota | 9 | 1.000 / 1.000 | 1.000 / 1.000 | 1.000 / 1.000 |
| crashed_to_shell | 1 | 0.500 / 1.000 | 0.500 / 1.000 | 0.500 / 1.000 |
| error_loop | 4 | 1.000 / 1.000 | 1.000 / 0.750 | 1.000 / 0.500 |
| still_starting | 1 | — / 0.000 | 0.000 / 0.000 | 0.000 / 0.000 |

## Bands and gate

| Arm | Strong actionable | Weak overall | Observed thresholds | Coverage-qualified verdict |
| --- | ---: | ---: | --- | --- |
| Screen | 17/18 = 0.944 | 10/10 = 1.000 | both pass | Inconclusive; incomplete matrix |
| Context, current auth | 21/21 = 1.000 | 9/11 = 0.818 | both pass | Inconclusive; incomplete matrix |
| Context, first-run auth | 22/22 = 1.000 | 14/16 = 0.875 | both pass | Inconclusive; incomplete matrix |

## Provider and transport precision

| Group | Screen | Context, current auth | Context, first-run auth |
| --- | ---: | ---: | ---: |
| Claude (27 frames) | 0.815 | 0.778 | 0.926 |
| Codex (20 frames) | 1.000 | 1.000 | 1.000 |
| Cursor (4 frames) | 1.000 | 1.000 | 1.000 |
| OpenCode (9 frames) | 0.667 | 0.778 | 0.778 |
| `pty` (60 frames) | 0.867 | 0.867 | 0.933 |
| `chat_backend` | no frames | no frames | no frames |

Per-band precision by provider (`strong / weak / ignored`):

| Provider | Screen | Context, current auth | Context, first-run auth |
| --- | --- | --- | --- |
| Claude | 1.000 / 1.000 / 0.545 | 1.000 / 1.000 / 0.333 | 1.000 / 1.000 / 0.500 |
| Codex | 1.000 / 1.000 / — | 1.000 / 1.000 / — | 1.000 / 1.000 / — |
| Cursor | 1.000 / — / — | 1.000 / — / — | 1.000 / — / — |
| OpenCode | 0.000 / 1.000 / 0.500 | 1.000 / 0.500 / — | 1.000 / 0.667 / — |

Per-band precision by transport (`strong / weak / ignored`):

| Transport | Screen | Context, current auth | Context, first-run auth |
| --- | --- | --- | --- |
| `pty` | 0.946 (35/37) / 1.000 (10/10) / 0.538 (7/13) | 1.000 (40/40) / 0.818 (9/11) / 0.333 (3/9) | 1.000 (40/40) / 0.875 (14/16) / 0.500 (2/4) |
| `chat_backend` | no frames | no frames | no frames |

## Auth criterion A/B

On the nine auth-labelled frames, the current criterion produced precision
1.000 and recall 4/9 (0.444). The first-run-aware criterion produced precision
1.000 and recall 9/9 (1.000). The first-run-aware variant wins this observed
A/B and is saved as `criteria.json`; the incomplete overall matrix still blocks
a qualified runtime gate.

## Missing coverage

The 33 missing groups are reported in each ignored `results-*.json` output.
Notable gaps are Codex update (1/4) and crash (1/4); Claude working (2/4),
Codex login/error/startup, OpenCode update/trust/login/model/crash/error/startup,
most Cursor labels, and every LM Studio label. The capability catalog and plugin
list on this instance exposed no LM Studio chat-backend provider, so transport
coverage could not be collected without an install, which was not authorized.
