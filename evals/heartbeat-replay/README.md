# Team heartbeat replay

Run only in the Orchestrator's canonical checkout after a real merge:

```sh
python3 evals/heartbeat-replay/run.py --plan pl_blazzd --arm fixture
python3 evals/heartbeat-replay/run.py --plan pl_blazzd --arm no-model
```

The runner calls `build.sh test-one app heartbeat_gate_replay_<arm>` with
`RUST_TEST_NOCAPTURE=1`; it executes the production Rust gate, not a Python
reimplementation. A missing result, zero ticks, a failed bounded runner, any
missed incident or any unexpected wake fails. Each tick prints its
wake/suppression, the policy expectation and reason, then one JSON result
reports counts, `missed_incidents` and `unexpected_wakes`. Record both results
as a plan stride. Run one canonical gate at a time.

The checked-in fixture is a conservative reconstruction from the owner-supplied
Orchestrator transcript export and persisted plan/task strides. Of 49 supplied
records, 46 have the leading daemon team-heartbeat envelope. Two are owner-set
reminders and one is source inspection output; their exclusion is recorded.
The fixture retains timestamps, delivered-text hashes, stride identities and
content hashes rather than whole delivered prompts or Worker report bodies.
Cumulative reconstructed plan projections are compacted into a content hash;
their original delta keys are retained. No prompt is asserted at a historical
tick: prompt presence and the other missing health fields remain unknown.

The export spans 2026-09-30 through 2026-10-02. It contains no historical
per-tick Supporter journal, worker ACK/binding/process/turn/commit evidence, or
exact incident onset and recovery intervals. Those fields remain unknown.
The five-worker update-prompt stall is evidenced by `ms_h0j6uz`; Muse's lost
report by `ms_trqs1n`; Keyless's lost report by the owner's assignment. Possible
incident intervals conservatively cover the captured window. Plan deltas are
reconstructed by stride timestamp, with transition/report strides labeled as
incidents. Automatic reminders need a connected Supporter's approving choice;
hard incidents are observation input and never force a wake. The expectation is
derived from that policy and the snapshot identity, not from the gate's verdict:
the `no-model` arm wakes no tick, and the `fixture` arm (an approving fixture)
wakes once for each tick whose observation differs from the last decided one
and never for an unchanged one. A tick with an empty delta, the same plan
projection, worker health and learnings, and no new hard event is unchanged:
an accepted reminder only moved its baseline. A miss is an approved new observation that did not
wake. These counts are **not measured savings**, and this replay does not prove
historical signal capture, incident onset timing or model judgment quality.
Denial, uncertainty and identical-snapshot suppression are covered by the
synthetic Rust scenarios separately.

Current ticks journal only bounded diagnostics in the private Supporter
journal: reason, verdict, `decided_by`, observation id, state hash, reminder
kind, scoped ids and adapter metadata, never the snapshot itself. The public
Settings and Team feeds expose only allowed labels/actions/reasons. A future
true replay can export those observations into the same `ticks` schema and use
`--input <fixture.json>` with independently labeled incidents. Keep the
labels separate from the gate's verdict so an incorrect decision cannot become
its own ground truth; such an export needs its own scrubbed observation source. No model connection, network call, sleep or poll is
required by either replay arm.
