# Passive captured observation correlation

Scope: the existing connected tab-state observer and scrape-candidate producer.
This schema supplies diagnostics to Probe; it supplies no action authority.
Qualification remains disabled. No disconnected producer is added.

## Journal and feed schema, version 2

The existing `observation_id` and `decided_by` remain at the top level.
A captured record adds `decision_point`, `observation_status`, `phase`,
`correlation`, `model_verdict` and `alternate_verdict`. An uncaptured record
omits `correlation` and `decision_point` and reports `observation_status:
unavailable`.

| Field in `correlation` | Meaning |
| --- | --- |
| `version` | `2` |
| `decision_point` | The original captured trigger, never a later inferred label |
| `producer` | `lifecycle_event`, `output_candidate`, `parked_edge`, `prompt_established` or `prompt_confirmation` |
| `parent_observation_id` | Actual causal parent capture id for `prompt_established` and `prompt_confirmation`; otherwise null |
| `observation_id` | The original capture id, equal to the journal id |
| `launch_id` | SHA-256 of the captured local launch key, which includes the PTY and launch nonce; captured metadata generation is used when local identity is unavailable |
| `session_id` | SHA-256 of the captured claimed session, or null if unavailable |
| `occurrence_id` | SHA-256 of the serialized `[launch_id, session_id, occurrence]` tuple |
| `occurrence` | The existing coordinator occurrence number |
| `frame_hash` | The existing full captured transport frame hash, as 16 hexadecimal characters |
| `candidate_evidence` | `observed` with `source: provider_safe_target` and `kind: update` or `onboarding`, only when the already-executed metadata producer carried that result; otherwise `unavailable` |

Covered triggers: `launch_ready`, `startup_deadline`, `startup_escalation`,
`prompt_candidate`, `worker_idle_without_report`, `busy_without_output`, `tab_exit`.
Captured cancellation, queue rejection and stale/disconnected rejection retain
the original receipt. A failure before a successful capture cannot invent one.

`observation_status: completed` means the existing classifier consumed a parsed
response, including `other` and low-confidence fallback. `rejected` means work
was discarded or failed. A valid Q2 label returned before rejection can still
appear in `model_verdict`; it does not make the observation completed.

`phase` separates `classification_completed`, `classification_rejected`,
`action_authorized` and `action_stale`. Classification and action records for a
single captured decision retain the same correlation and observation id. An
action-authorized record proves only that the existing responder's immediate
revalidation passed; it does not prove PTY delivery or provider completion.

`model_verdict` contains only a genuinely returned, criteria-allowed Q2 label
and its finite confidence in [0, 1], with `status: observed`; otherwise its
status is `unavailable`. It is not a safe-choice, key sequence or action proof.
The existing top-level classification fields keep their existing meanings.

`alternate_verdict` is `skipped` when the connected adapter decided the
classification, or `unavailable` when the subsequent heuristic result is not
carried to this journal seam. No heuristic is rerun for diagnostics and no
paired label is synthesized from the model verdict.

Feed projection accepts only this bounded schema, known triggers, opaque hashes
and matching outer/nested observation id and point. Malformed receipts are
omitted and their observation status becomes unavailable. Extra raw fields and
unknown verdict labels are not projected. Existing fallback sources set the
feed fallback flag independently of the narrative reason.

## Probe evidence limits

The capture id is the duplicate barrier for replay of that receipt. It is not
proof that two callbacks came from different producer events. `prompt_established`
is issued once after a model-classified prompt is published; its first completed
eligible vote requests one actual `prompt_confirmation` capture. Both have
distinct capture ids and can have the same occurrence and frame. Confirmation
does not count as new provider output. Probe must bind each receipt to its
point, parent, launch/session and occurrence rather than infer a successful
action from a classification record.

The generic scrape/trust predicate result is not carried by existing metadata.
It remains unavailable here. Safe-target
candidate evidence describes an existing parser result, not an alternate
full-tab classification. Fallback callbacks execute after this record and do
not return a verdict to it, so a paired fallback classification is unavailable.

The safe-response action serializer now retains the exact captured envelope and
labels its phase without claiming that the provider consumed a key. Supporter
ActionRecord and recovery rows without an envelope still do not gain an invented
receipt. No disconnected capture or extra heuristic is added.

Pure coverage includes same-occurrence/static-frame captures, causal-parent
validation, receipt replay,
reused-PTY nonce and claimed-session scoping, wrong point/id/schema rejection,
secret-free serialization, feed correlation and unavailable alternate data.
In-memory observer/candidate tests exercise the actual journal/feed seams,
retain returned model data through fallback and rejection, and require no
extra screen reads or disconnected journals. The cfg(test)-only
`safe_response/qualified_tests.rs` uses an actual PTY child and checks the final
dialog state. Its fixture qualification is not a live model measurement, and
the test has not executed in the source-only lane.
