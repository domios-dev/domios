# evals/plugins — plugin lifecycle (install · update · work)

Validates that CodeTerm's channel plugins **load, register their capability, run, and
survive version updates** — the question "do my plugins still work after I publish a new
version?" Covers the two shipped capability plugins: `transcriber` and `bitwarden`.

## What's already covered by unit/integration tests (run with `./build.sh test`)
- `plugin::js_runtime::loader::capability_plugin_version_update_and_compat_gate` —
  install v0.1.0 → update to v0.1.1 → **capability survives + new behaviour active** →
  incompatible `hostApi` is **gated** (daemon survives). This is the core update flow.
- `plugin::js_runtime::transcriber::shipped_transcriber_transcribes_via_fake_daemon` —
  the real `plugins/transcriber/plugin.js` loads → `host.fetch` → fake daemon → text.
- `plugin::js_runtime::secret_backend::shipped_bitwarden_*` — the real
  `plugins/bitwarden/plugin.js` loads as a `secret_backend`; pure logic (base64, error
  mapping, envelope unwrap, cipher build) is **parity-checked against the native Rust**.

## Live daemon checks — `./run.sh` (run against dev on :7686)
Requires dev up: `./build.sh dev --wait`. Uses the bench lib (`dc`, locked to dev).

1. **Loaded + registered** — greps `~/.codeterm-dev/logs/codeterm.log` for
   `transcriber registered as transcriber` and `bitwarden registered as secret backend`.
2. **Transcriber works through the real daemon** — starts a fake transcribe daemon, points
   the daemon's `CODETERM_TRANSCRIBE_URL` at it (set before `build.sh dev`), POSTs a voice
   attachment to `/api/transcribe-voice`, asserts the returned `-=-codeterm:voice-=-` marker
   carries the transcript.

## Version-update drill — `./update-drill.sh`
Proves a published new version is picked up and still works:
1. Install `transcriber` v0.1.0 into `~/.codeterm-dev/plugins/transcriber/` (copy + set version).
2. Restart dev; assert it loads at 0.1.0 and transcribes.
3. Bump the copy to v0.1.1 with a marker change; restart dev.
4. Assert the log shows 0.1.1 and the new behaviour is active — **no break**.
5. Write a copy with `minCodeterm` far in the future / bad `hostApi`; restart; assert it is
   **skipped** (log line) and the daemon still boots and the other plugins still work.

## Notes
- Full plugin-list assertion over HTTP lands with Phase 4 (web parity for `plugin_manager_*`).
  Until then these checks use logs + observable behaviour, which is sufficient to prove
  install/update/work/gate.
- Bench safety: never invoke `codeterm` directly — use `dc` (locked to dev :7686).
