# Auto-update testing playbook

How to exercise CodeTerm's self-update pipeline without shipping a real
release. Mirrors `docs/plans/auto-update.md` §Testing — read this when you
need to actually run the rig; read the plan when you need design rationale.

Cross-references:
- [Plan](../plans/auto-update.md)
- [Signing setup](./auto-update-signing.md)
- [Test fixture README](../../tests/fixtures/auto-update/README.md)

## Overview

You cannot ship a self-update feature without proving it works without
bricking real installs. The dev instance (`~/.codeterm-dev/` on this
machine) is the staging ground. The `dev-updater-allowlocal` Cargo feature
flag whitelists `file://` and `http://127.0.0.1:*` manifest URLs; release
builds reject them so production can never load an unsigned local
manifest.

All the test surfaces below assume the backend was built with that
feature:

```bash
bun run tauri build --features dev-updater-allowlocal
```

## 1. Local manifest override

The CLI flag lives at `cli/src/main.rs` (`Update { manifest_url, ... }`):

```bash
codeterm update --check --manifest-url file:///tmp/test-manifest.json
codeterm update --apply --manifest-url file:///tmp/test-manifest.json
```

Both forms reach the backend's `/api/update/check` and
`/api/update/install` endpoints with the override in the query string.
The override applies for that single call only — channel state is
unchanged.

`file://` manifests load synchronously off disk. `http://127.0.0.1:*`
manifests are fetched via `reqwest`. Both pass through
`update::manifest::fetch`, which gates them on
`#[cfg(feature = "dev-updater-allowlocal")]`.

## 2. Local asset server

Keep everything off the network so you don't fight CI rate limits or
flaky GitHub mirrors:

```bash
cd tests/fixtures/auto-update
python3 -m http.server 8080
```

Manifest URL becomes `http://127.0.0.1:8080/manifest.json`; assets resolve
to `http://127.0.0.1:8080/dist/<name>`. `is_loopback_http` in
`update::manifest` accepts only `127.0.0.1` and `localhost`.

## 3. Version inversion trick

Build two CodeTerm versions back to back, treating the lower one as the
"installed" build and the higher one as what the manifest advertises:

```bash
# managed by tests/fixtures/auto-update/build-fixture.sh
sed -i 's/"version": ".*"/"version": "0.0.1"/' src-tauri/tauri.conf.json
bun run tauri build --features dev-updater-allowlocal -- --target x86_64-pc-windows-msvc

sed -i 's/"version": "0.0.1"/"version": "0.0.2"/' src-tauri/tauri.conf.json
bun run tauri build --features dev-updater-allowlocal -- --target x86_64-pc-windows-msvc
```

Run `tests/fixtures/auto-update/build-fixture.sh` — it does the above
plus regenerates `manifest.json` with the real signatures via
`regenerate-manifest.py`.

End-to-end: install 0.0.1, launch it, run `codeterm update --apply
--manifest-url http://127.0.0.1:8080/manifest.json`, watch the dev
instance upgrade itself. Verifies binary swap, signature check, restart,
session resume in one go.

## 4. Signature negative path

Tampering must be rejected:

1. Build the rig per §3.
2. Flip a byte in one asset:
   ```bash
   printf '\x00' | dd of=tests/fixtures/auto-update/dist/codeterm-0.0.2-test-x64.msi.zip \
     bs=1 count=1 seek=4096 conv=notrunc
   ```
3. Do **not** regenerate `manifest.json` — keep the old (now stale) signature.
4. Run `codeterm update --apply --manifest-url http://127.0.0.1:8080/manifest.json`.

Expected: updater aborts, logs `signature verification failed`, leaves
the old binary in place. The `update-smoke` CI job covers the happy path;
this test stays manual until we have a parallel job that runs it as a
must-fail.

## 5. Pane survival

The make-or-break check. Steps:

1. Launch the OLD-version CodeTerm.
2. Spawn 2-3 panes with long-running processes — Claude/Codex agents
   mid-conversation if you can, otherwise any PTY task that keeps
   producing output.
3. Snapshot pane state via `GET /api/panes`.
4. Trigger `codeterm update --apply --manifest-url http://127.0.0.1:8080/manifest.json`.
5. Wait for relaunch — `/api/health` exposes the running build hash and
   PID.
6. Snapshot again. Pane count, titles, CWDs must match. If the panes had
   `agent_session_id` set, the resumed panes must carry the same id.

The pytest version of this lives at `tests/e2e/test_update_resume.py` and
is gated behind `CODETERM_RUN_UPDATE_RESUME=1` plus
`CODETERM_TEST_MANIFEST_URL=<url>`. The shape-check portion of that file
runs unconditionally so PRs that break the pane snapshot payload fail
locally before they ever reach the rig.

## 6. Failure modes

Prove these by hand before any v1 ship:

- **Network drops mid-download** — disable wifi after `download_and_install`
  starts. Updater retries 3× with backoff (handled by
  `tauri-plugin-updater`), then aborts cleanly. No half-applied state.
- **Disk full during extract** — fill the install volume before triggering
  apply (`fallocate -l <size>` on Linux, `New-Item -Force` on Windows).
  Apply must abort, leave old binary, surface error.
- **Crash on first launch of NEW version** — manually corrupt the new
  binary post-swap (chmod 000, rename, etc.). User reinstalls the old
  asset by hand; rollback is out of scope for v1.
- **Concurrent `--apply`** — fire two `codeterm update --apply`
  invocations within the same second. Second one detects lock and
  refuses. Backend lock lives in `update::apply`.

## CI smoke job

`.github/workflows/update-smoke.yml` runs the version-inversion happy path
on labeled PRs (`test:update-smoke`) and on manual dispatch. Steps:

1. Checkout, install Rust + Node + bun.
2. Run `tests/fixtures/auto-update/build-fixture.sh` — builds OLD + NEW
   with the dev feature flag.
3. Install the OLD version locally.
4. Serve `tests/fixtures/auto-update/` over `python -m http.server 8080`.
5. Launch CodeTerm OLD, run `codeterm update --apply --manifest-url
   http://127.0.0.1:8080/manifest.json`.
6. Poll `/api/health` until the build hash flips, then check
   `codeterm --version` reports the NEW version.

Several pieces of the workflow are stubbed with `TODO:` — see the
workflow file. Production-grade smoke runs need a headless launch
mode and a `--no-restore` CLI flag that does not yet exist. Those
prerequisites land before this job is enabled by default.

## Manual sign-off checklist

Before promoting `feat/auto-update` to `develop`:

- [ ] Local manifest override works on Windows.
- [ ] Local manifest override works on macOS Apple Silicon.
- [ ] Local manifest override works on macOS Intel.
- [ ] Tampered asset is rejected (negative test, §4).
- [ ] Live Claude pane survives update with conversation intact.
- [ ] `mem.db` is intact (run a `mem search` post-update; row count matches pre-update).
- [ ] No lingering processes from old binary (`tasklist | grep codeterm` shows only the new PID on Windows; equivalent on macOS).
- [ ] `codeterm update --check` is reliable on flaky network (kill wifi mid-call → graceful timeout, not hang).
