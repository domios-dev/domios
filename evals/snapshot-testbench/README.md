# evals/snapshot-testbench — snapshot library gate

Replays every case in the screen snapshot library (`testbench/`) through agent detection and the bundled
provider plugins, and requires the gating cases to stay green. It is a curated eval, not part of the CI
test suite: run it when detection, the signal extractor or a provider plugin changes.

## Run

```bash
evals/snapshot-testbench/run.sh
```

The runner executes the ignored `snapshot_testbench_library_is_green` test in `codeterm-devtools` with
`--include-ignored --nocapture`, printing per-case results and a scorecard. A missing library or any red
gating case fails the run.
