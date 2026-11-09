# Secret-audit addenda

This adds to the original secret audit recorded under plan `pl_7lsisa`, task
`task_zmyr1n` (Sentinel worker report, 2026-10-23). Values are not reproduced.

| Fingerprint | Verdict | Action |
| --- | --- | --- |
| `e8a91318bfd8c897e8e88dbc122ebe05bc06e7be:plugins/browser/tests/blueprintStep.test.ts:generic-api-key:72` | synthetic redaction fixture in a Browser node test, never a credential | Exact historical fingerprint ignored; current fixture constructs its token at runtime. Redaction assertions retained. |

The fixture supplies synthetic page text and a final URL to a fake Browser
surface. It never authenticates a request. History remains intact; no blanket
path or rule exemption is added. Local scan results are recorded in the
integration report after verification.

## Provider output-limit comparisons

| Fingerprint | Verdict | Action |
| --- | --- | --- |
| `5a57f7e202bb85820fb933e6dded30656671860b:plugins/provider-claude/plugin.js:generic-api-key:667` | Protocol field comparisons, not a credential | Ignore this exact historical fingerprint. |
| `5a57f7e202bb85820fb933e6dded30656671860b:plugins/provider-codex/plugin.js:generic-api-key:1178` | Protocol field comparisons, not a credential | Ignore this exact historical fingerprint. |
| `096608ee249126d0e1271b3910d43bbb2cc704d8:plugins/provider-codex/plugin.js:generic-api-key:2268` | Same comparison after bundle regeneration | Ignore this exact historical fingerprint. |
| `plugins/provider-claude/plugin.js:generic-api-key:1010` | Same comparison in a directory scan | Ignore this exact file/line fingerprint. |
| `plugins/provider-codex/plugin.js:generic-api-key:2268` | Same comparison in a directory scan | Ignore this exact file/line fingerprint. |

Directory-scan fingerprints follow the current bundle lines; the superseded
lines 667 and 1178 no longer exist in the tree.

Both generated lines compare `stop_reason` with `max_tokens` and `finish_reason`
with `length` inside `outputLimit`. Neither contains a credential or performs
authentication. The scanner reports these comparisons as `generic-api-key`
findings. Source, history, scanner rules and all other findings remain subject
to the same checks.
