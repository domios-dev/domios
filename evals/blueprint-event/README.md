# Blueprint event fixture

Run `CODETERM_INSTANCE=bpdev CODETERM_API_PORT=<named-dev-port> bash evals/blueprint-event/live.sh` from the checkout with that named DEV instance already running. The script sources the bench library and verifies the event stream before installing the local fixture plugin and creating the tab-owned pipeline in `pipeline.yaml`.

The oracle checks one delivered event, a delivered result, exactly one completed run, and no model ask step. Evidence is written to `transcript.md`. The EXIT trap stops the Blueprint, closes its owner tab, and terminates the event-stream process. The fixture plugin remains installed in this disposable DEV instance.
