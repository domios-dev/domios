# Tab-state Jev measurements

Captured and measured 2026-10-01 on the named `tabstate` dev instance. The
Orchestrator's revised coverage rule requires four real labelled frames pooled
per label and both `pty` and `chat_backend` transports. Provider and transport
precision are reported, not gated. The completed dataset has 82 human-labelled
frames and support for every label. Each of the three runs used 82 calls, for
246 successful calls total; every run stayed below the 300-call ceiling.

## Per-label precision / recall

`—` means the model made no prediction for that label, so precision is
undefined. Support is the number of labelled frames.

| Label | Support | Screen P / R | Context, current P / R | Context, first-run P / R |
| --- | ---: | ---: | ---: | ---: |
| ready_idle | 25 | 0.793 / 0.920 | 0.793 / 0.920 | 0.958 / 0.920 |
| working_normally | 12 | 0.846 / 0.917 | 0.846 / 0.917 | 0.846 / 0.917 |
| provider_update_prompt | 4 | 1.000 / 1.000 | 1.000 / 1.000 | 1.000 / 1.000 |
| trust_or_permission_prompt | 10 | 1.000 / 1.000 | 1.000 / 1.000 | 1.000 / 1.000 |
| login_or_auth_required | 9 | 1.000 / 0.444 | 1.000 / 0.444 | 1.000 / 1.000 |
| model_unavailable_or_quota | 10 | 1.000 / 1.000 | 1.000 / 1.000 | 1.000 / 1.000 |
| crashed_to_shell | 4 | 0.500 / 1.000 | 0.500 / 1.000 | 0.500 / 1.000 |
| error_loop | 4 | 1.000 / 1.000 | 1.000 / 1.000 | 1.000 / 1.000 |
| still_starting | 4 | — / 0.000 | — / 0.000 | — / 0.000 |

## Bands and gate

| Arm | Strong actionable | Weak overall | Gate verdict |
| --- | ---: | ---: | --- |
| Screen | 24/28 = 0.857 (fail) | 15/15 = 1.000 (pass) | Fail |
| Context, current auth | 27/27 = 1.000 (pass) | 13/17 = 0.765 (pass) | Pass |
| Context, first-run auth | 26/26 = 1.000 (pass) | 23/28 = 0.821 (pass) | Pass |

The complete coverage-qualified gate requires strong actionable precision of at
least 0.90 and weak overall precision of at least 0.60. Strong and weak
thresholds are confidence >= 0.80 and 0.60-0.79, respectively. Screen-only
fails on strong precision. Both context arms pass both bands.

## Provider and transport precision

| Group | Screen | Context, current auth | Context, first-run auth |
| --- | ---: | ---: | ---: |
| Claude (31 frames) | 0.742 | 0.742 | 0.903 |
| Codex (27 frames) | 1.000 | 1.000 | 1.000 |
| Cursor (4 frames) | 1.000 | 1.000 | 1.000 |
| LM Studio (6 frames) | 0.667 | 0.667 | 0.667 |
| OpenCode (14 frames) | 0.857 | 0.857 | 0.857 |
| `pty` (76 frames) | 0.868 | 0.868 | 0.934 |
| `chat_backend` (6 frames) | 0.667 | 0.667 | 0.667 |

Per-band precision by provider (`strong / weak / ignored`):

| Provider | Screen | Context, current auth | Context, first-run auth |
| --- | --- | --- | --- |
| Claude | 0.833 / 1.000 / 0.545 | 1.000 / 0.800 / 0.300 | 1.000 / 0.889 / 0.667 |
| Codex | 1.000 / 1.000 / 1.000 | 1.000 / 1.000 / 1.000 | 1.000 / 1.000 / — |
| Cursor | 1.000 / — / — | 1.000 / — / — | 1.000 / — / — |
| LM Studio | 0.000 / 1.000 / 1.000 | — / 0.333 / 1.000 | — / 0.333 / 1.000 |
| OpenCode | 0.600 / 1.000 / 1.000 | 1.000 / 0.833 / 0.000 | 1.000 / 0.800 / 1.000 |

Per-band precision by transport (`strong / weak / ignored`):

| Transport | Screen | Context, current auth | Context, first-run auth |
| --- | --- | --- | --- |
| `pty` | 0.900 (45/50) / 1.000 (12/12) / 0.643 (9/14) | 1.000 (50/50) / 0.857 (12/14) / 0.333 (4/12) | 1.000 (44/44) / 0.880 (22/25) / 0.714 (5/7) |
| `chat_backend` | 0.000 (0/2) / 1.000 (3/3) / 1.000 (1/1) | — / 0.333 (1/3) / 1.000 (3/3) | — / 0.333 (1/3) / 1.000 (3/3) |

## Auth criterion A/B

On the nine auth-labelled frames, the current criterion produced precision
1.000 and recall 4/9 (0.444). The first-run-aware criterion produced precision
1.000 and recall 9/9 (1.000). The first-run-aware variant wins this observed
A/B and is saved as `criteria.json`; it also passes the complete-set context
gate with strong actionable precision 1.000 and weak precision 0.821.

## Gate coverage and captures

Pooled support is `ready_idle` 25, `working_normally` 12,
`provider_update_prompt` 4, `trust_or_permission_prompt` 10,
`login_or_auth_required` 9, `model_unavailable_or_quota` 10,
`crashed_to_shell` 4, `error_loop` 4, and `still_starting` 4. Transport support
is `pty` 76 and `chat_backend` 6.

The three added Codex update frames each showed the visible update prompt and
were labelled `provider_update_prompt` by Watchman. Codex fresh-folder captures
also showed folder trust and were labelled accordingly. Claude's update check
showed no available-update prompt. The added crashed-to-shell frames followed
provider interruption; the still-starting frames showed the launcher before a
composer appeared. LM Studio frames covered idle, answering, and a visible
HTTP model error. Labels were assigned from the screens, not model output.
