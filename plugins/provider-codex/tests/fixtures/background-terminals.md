# Codex background terminal capture

`background-terminals.json` projects thread/start and background terminal traffic from the installed Codex CLI 0.162.1, recorded through `tests/capture-runtime.mjs background-terminals` using gpt-6-luna. Cwd and home paths were recursively sanitized. Account data, turn prose, and unrelated notifications are omitted. Expectations in runtime-background.test.ts were authored from the recorded envelopes.

The command started a local ephemeral listener. The observed sequence was a nonempty list, terminate acknowledged true, an empty list, clean acknowledged, and an exact missing-thread error (-32600, `thread not found: <requested thread id>`, no data). No other error shape authorizes absence.

The first capture wrote its traffic but exited 1 while removing its temporary cwd (EBUSY). An OS inspection then found the exact Node command still running as PID 29240 with parent PID 53156, despite the empty list. The Worker verified the command fingerprint, terminated that specific process, waited for exit and confirmed absence. The empty temporary directory was subsequently removed. This is evidence that an acknowledged terminate, and even the server roster, cannot replace host process ownership. The recorder now ties its background command to a harness-owned control socket and waits for its own server exit before removing cwd.

The host's Windows Job and Unix process-group lease are not exercised by this standalone recorder. Canonical live QA must demonstrate cleanup of an owned runtime, including actual process absence. This fixture alone does not establish that acceptance criterion.

`background-terminals-controlled.json` is a second capture after that harness repair. It completed with exit 0. Its background command connects to a harness-owned local socket and exits when the connection closes. The same nonempty-list / terminate / empty-list / clean / exact-missing-thread sequence was observed. A subsequent OS command-line query found no remaining controlled command. Both recordings are replayed against source and bundled hooks.
