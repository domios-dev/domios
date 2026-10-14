# Upgrade restore release blocker

This bench targets the BLACKSTAR 1.14.7 runtime upgrade failures: duplicated
Manager prompt/identity exceeding the 64 KiB desk leaf, Codex history crossing
256 KiB continuation state, and legacy Workers without an attempt binding.

Capture is read-only against PROD and deliberately lives outside `evals/`:

```bash
node scripts/capture-upgrade-desk.mjs /tmp/upgrade-desk.json latest /path/to/current/codeterm
```

The CLI surface is `sessions backups export <timestamp|latest> --sanitize --out
<file>`. `latest` reads the live mux without saving, dispatching or loading a
provider runtime. The timestamp form reads a saved backup. Neither writes to
instance storage. The client refuses output paths within instance directories.
The daemon masks text with filler preserving UTF-8, UTF-16 and JSON escaped
lengths. It keeps role/profile/provider, models, session and assignment ids,
structure and scalar state, captures the typed Plan store with tasks and
execution attempts, and keeps live hidden/tabbar metadata separately. Unsupported
sanitization or missing transcript data refuses capture.

Histories are generated from normalized parsed transcript shape with filler
content, not copied provider session files. `history_pages` means normalized
100-item capture pages; it does not claim a provider-native page count.
The fake app-server converts these rows into Codex `thread/turns/list` and
`thread/items/list` responses. It never opens a real provider session and refuses
unknown threads and mutation methods. No provider account credentials are needed.

## Canonical procedure

Workers do not build or launch DEV. The Orchestrator runs this sequence in the
warm canonical checkout, in Team-bound dev/QA utility shells. Follow the dev-run
skill for the foreground `./build.sh dev` lifecycle. Use a disposable named
instance beginning `upgrade-restore-` and an unused non-production port. The
bench library checks the instance identity on every CLI call. No bare CLI calls
are allowed within the bench, and no helper that polls/sleeps is used.

Prepare the real capture and separately generate/prepare worst cases:

```bash
bash evals/upgrade-restore/run.sh prepare /tmp/upgrade-desk.json /tmp/upgrade-real
node evals/upgrade-restore/fixtures.mjs /tmp/upgrade-worst.json "$PWD" /tmp/upgrade-worst/project
bash evals/upgrade-restore/run.sh prepare /tmp/upgrade-worst.json /tmp/upgrade-worst
```

Start the target build with the prepared `bin/` directory first in its PATH,
for example `/tmp/upgrade-real/bin`, and `CODETERM_INSTANCE=upgrade-restore-real`.
Use the same named instance and port for its subsequent restart. The daemon's
provider descriptor must resolve `codex` to the prepared fake executable;
verification refuses a real executable before touching any provider session.
`prepare` rewrites replay working directories into its isolated project while
retaining the size evidence in the original capture.

With that target DEV running, stage only through the dev wrapper:

```bash
CODETERM_INSTANCE=upgrade-restore-real CODETERM_API_PORT=<dev-port> \
  bash evals/upgrade-restore/run.sh stage /tmp/upgrade-real/desk.json <health-build_sha-full> /tmp/upgrade-real
```

Import refuses PROD, an unnamed DEV, or a desk owning PTYs. It imports the typed
desk and sanitized Plan store and returns `restart_required`. A 200 response is
not a restore verdict. Stop/start this named DEV using the dev-run skill so the
current build loads committed storage, then require ready state and history:

```bash
CODETERM_INSTANCE=upgrade-restore-real CODETERM_API_PORT=<dev-port> \
  bash evals/upgrade-restore/run.sh verify /tmp/upgrade-real/desk.json <health-build_sha-full> /tmp/upgrade-real 90
```

Repeat with `upgrade-restore-worst`, its isolated port and `/tmp/upgrade-worst`.
Each verifier has one bounded failure deadline (maximum 120 seconds). It waits
for CLI/process completion and provider runtime protocol handles, not elapsed-time
readiness. It checks build SHA, every captured leaf's role/provider/session,
confirmed runtime nonce, loaded runtime epoch, Worker attempt identity, visibility
and consumed fake history. It also requires a changed daemon boot id and the
SHA256 of the staged fixture, so import without restart cannot pass. Failures name their tab and exact runtime reason,
including SizeLimit, binding incomplete, missing tab and deadline. Evidence is
`replay-result.json` and `provider-trace.jsonl` in the prepared root.

For the pre-fix comparison, use a build retaining the bench exchange API and CLI
instrumentation but excluding the three product repairs. Record that exact SHA
and which repair commits are absent; an old binary lacking instrumentation cannot
prove the regression. Expected failure is nonzero with the three named causes.
The post-fix SHA must pass both the real capture and worst cases. Store all four
receipts with the release evidence. Do not push an app release while any backing,
required ready state or receipt is missing. Frontend/plugin-only releases are
outside this app startup gate.

## Focused checks and current limitations

```bash
node --test evals/upgrade-restore/model.test.mjs evals/upgrade-restore/process.test.mjs
```

Canonical Rust selectors: `./build.sh test-one app api::session_exchange` and
`./build.sh test-one cli cmd::manifest`; rebuild CLI with `./build.sh cli` to
regenerate its contract artifacts before the manifest check.

The first implementation supports the official Codex runtime protocol. A capture
containing other providers fails closed before replay. Hidden tabs omitted by
Domios's snapshot writer also fail closed because no persisted replay backing
exists. These failures require an owner decision or added backing; they are not
skips, and unit fixtures are not pre/post DEV proof. The capture command requires
a daemon that exposes the new export route; a new CLI cannot invent that route in
an older running PROD daemon. Read-only export compatibility and cross-platform
live replay remain canonical integration work.
