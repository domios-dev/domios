# Blueprint command pipeline

Run `bash evals/blueprint-pipeline/live.sh` from the checkout against a running DEV instance. For a named instance, export its `CODETERM_INSTANCE` and `CODETERM_API_PORT` first. The bench library resolves and verifies the connection; do not invoke the CLI directly from this bench.

The fixture creates a tab-owned Blueprint from `echo.yaml`, waits on its event stream, and checks that both steps and their journal entries reach `done`. It prints the completed pipeline id; retain the shell output as evidence. The EXIT trap stops the Blueprint, closes its owner tab, and terminates the event stream.
