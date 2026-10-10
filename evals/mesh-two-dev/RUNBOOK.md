# Runbook: two DEV consoles over mesh

How to join one Mac DEV and one Windows DEV over the Domios mesh and prove that
remote ("managed by") tabs, mesh operations and the web UI peer picker work on
every provider. The acceptance contract is spec `sp_apg34f`, section "Two DEV
consoles over mesh". The harness lives here (`evals/mesh-two-dev/`, checks B1
and B4) and in `evals/mesh-webui/` (checks B2 and B3). `README.md` in each
folder documents the scripts; this file is the operator procedure.

## Rules that never bend

- **One DEV per machine.** The Mac uses one named DEV (for example `aiderqa`,
  API 7697). Windows uses its single DEV (API 7686). Do not start extra DEV
  instances for a test; stop any you started.
- **PROD is never touched** (7685 on either machine): no kill, no restart, no
  pairing with a PROD identity, no config change.
- **Never reload a desktop window you do not own.** On the other machine, drive
  only the DEV, and only through the mesh or that machine's own operator.
- **Bench safety:** drive a DEV only through the bench lib (`source
  evals/_lib/lib.sh`, then `dc ...`). A bare `codeterm` from an agent tab
  inherits PROD routing.
- **Cheap models only** for test traffic: Claude haiku, Codex gpt-6-luna,
  OpenCode deepseek/deepseek-flash, Grok grok-4.6.
- **Proof comes from the provider's own record** for that tab's session
  (Claude/Codex JSONL, OpenCode SQLite read-only, Grok `updates.jsonl`). A UI
  label, CLI acknowledgement or screen text is never model or turn proof.

## 1. Prerequisites

1. Both DEVs are built from the same release commit. Check with
   `curl -s http://127.0.0.1:<port>/api/health | jq .build` on each machine.
2. Each DEV listens on its own tailnet address. Set the opt-in, then restart
   only that DEV through its launcher:

   ```sh
   # Mac, from the checkout the DEV is built from
   export CODETERM_INSTANCE=aiderqa CODETERM_API_PORT=7697
   source evals/_lib/lib.sh
   dc config set api.mesh_tailnet_host '"<mac tailnet ipv4>"'
   ./build.sh kill && ./build.sh dev --wait --notify-tab "$CODETERM_TAB_ID"
   ```

   On Windows, the DEV owner sets `api.mesh_tailnet_host` in that DEV's
   config and restarts it with `.\build.ps1 dev --wait`. Any DEV build may set
   the option; production refuses it.
3. **Prove both listeners**, on each machine:
   - `dc config get api_listener` lists BOTH `127.0.0.1:<port>` and
     `<tailnet ip>:<port>` under `ready_addresses`;
   - `curl http://<tailnet ip>:<port>/api/health` returns 200 from the OTHER
     machine;
   - the DEV log shows `[api] listener <tailnet ip>:<port> accepted its first
     connection` followed by `REST API generation N health-proven`.
4. **Firewall allow.** A new ad-hoc-signed dev binary gets no inbound
   non-loopback connections until the owner allows it:
   - macOS: answer the "accept incoming network connections?" dialog for
     `~/.codeterm-dev-<instance>/target/debug/codeterm`, or System Settings >
     Network > Firewall > Options. One Allow covers later rebuilds at the same
     path; a new instance or target path needs a new Allow.
   - Windows: allow the Defender prompt for the DEV `codeterm.exe` on Private
     networks, or add an inbound rule for its TCP port from `100.64.0.0/10`.

## 2. Pair the two DEVs, and only them

1. On both DEVs: `dc mesh status` and `dc mesh peers`. A DEV that already lists
   paired peers from older experiments must be cleaned first (see Pitfalls),
   because the acceptance requires "paired only with each other".
2. Find the other DEV. If both DEVs use the SAME API port, UDP discovery works:
   `dc mesh broadcast` on one, `dc mesh discover` on the other. If the ports
   differ (the usual case: Mac 7697, Windows 7686), UDP discovery cannot see
   the peer, so connect by explicit address from the initiating DEV:
   `dc mesh connect <instance id> --addr <tailnet ip>:<port>`. Either way, pick
   the exact development identity by instance id, never by advertised name.
3. Approve on the receiving DEV: `dc mesh approve <instance id>`.
4. Verify on both: each lists exactly one paired peer, the other DEV, online,
   with the expected API port, and no pending incoming request. Record both
   outputs. `node evals/mesh-two-dev/identity.mjs` encodes this check.

## 3. Checks and pass criteria

Every check runs for every provider, in three views (the tab's home machine,
the "managed by <peer>" remote view on the other machine, and the web UI
switched to the other machine), in both directions.

| Check | Harness | Pass when |
|---|---|---|
| B1 mesh ops | `evals/mesh-two-dev` driver | `tab list --mesh`, send, get-text, agent report back across peers, and paste put/get work from both origins; paste round-trips byte-exact |
| B1 status/progress (spec check 1) | stream + rendered observers | Working while the agent runs, Idle/Done at turn end; sending dot, elapsed time and progress lines live; remote and web views agree with the home view within about 1 s; no stuck Working, no missed Done |
| B1 chat (spec check 3) | driver + provider records | 15-30 medium turns with code blocks and tool calls; Stop works; a cheap model switch is proven from the next turn's own record; history survives a view reload |
| B1 sync (spec check 4) | convergence oracle | after a restart of either DEV the view reconnects and reconverges with the same tab ids and ordered history; no ghost tab; a remotely closed tab disappears everywhere |
| B2 web UI | `evals/mesh-webui/live.sh` | from each DEV's web UI, the titlebar peer picker switches to the other DEV and back through the Gateway self entry; tab list, chat, composer, Stop, model switch and managed-by tabs work for every provider |
| B3 terminals (spec check 2) | `evals/mesh-webui/capture-terminals.sh` | a plain shell and an agent TUI render identically in home, remote and web views: no garbled or duplicated output; resize, scrollback, colors, cursor, alternate screen and Ctrl+C work |
| B4 sync robustness | `b4` sampler, `mesh offline`/`online` | after a mesh drop on one DEV and a restart of each DEV, no lost, duplicated or out-of-order events |
| B4 leaks (spec check 5) | `sampling.mjs` + stream census | over 15-30 minutes with several remote tabs, daemon memory, threads, handles, peer sockets, live SSE streams and renderer memory stay bounded on BOTH machines; report start vs end |

Fix rule: every fix is DRY. One path for local and remote tabs and for
desktop and web UI; no remote-only copies and no provider branches in core.

## 4. Collecting evidence

- Offline harness checks first: `bash evals/mesh-two-dev/run.sh offline`.
- Read-only baseline: `bash evals/mesh-two-dev/run.sh capture <windows peer id>`.
- Provider records: `home-export.py` exports only the tab's bound session
  (Claude/Codex JSONL, OpenCode SQLite in read-only mode, Grok
  `~/.grok/sessions/<url-encoded cwd>/<session id>/updates.jsonl`). It refuses
  sibling sessions and newest-file guesses.
- Live SSE census: `GET /api/debug/events/stats` returns `streams` with live,
  opened and closed counts per stream kind and scope (local, or peer-relayed
  with the peer instance id). Sample it on both DEVs at start, every 5 minutes
  and at the end, together with the OS samples from `sampling.mjs`.
- Results: `driver.mjs record RUN_DIR VERDICT_JSON` appends a verdict, and
  `driver.mjs render RUN_DIR` writes `results.json` and `results.md`. A FAIL
  keeps its exact repro (steps, tab and session ids, log lines) and is never
  overwritten by a later PASS in the same run.

All output goes under `target-check/evals/<run>/` in the Mac checkout.

## 5. Restarts during the test

A restart counts only if it is real: a different listener PID with a later
process start time. A launcher that reports "already ready" without a new PID is
a failed restart, not a pass. Each machine restarts only its own DEV, and only
through its owner (on Windows, the Windows operator).

## 6. Cleanup and unpairing

1. Close the test tabs on both DEVs.
2. Unpair: `dc mesh disconnect <instance id>` on both sides. Disconnect
   revokes the identity by default; lift it later only with
   `dc mesh unrevoke <instance id>`.
3. Remove the tailnet opt-in if it should not persist:
   `dc config set api.mesh_tailnet_host null`, then a scoped DEV restart.
4. Stop any DEV instance you started for the test; leave the machine with one
   DEV and its PROD untouched.

## 7. Pitfalls hit during the 1.14.0 run

- **Stale data directory.** Reusing an old `~/.codeterm-dev-<instance>` kept
  an old config (wrong port, no tailnet host) and two stale pairings from
  earlier experiments, so the DEV never listened on the expected port. Use a
  fresh instance name or a clean data dir, and never delete someone else's
  data without asking.
- **macOS firewall hold.** The tailnet socket accepted TCP connections in the
  kernel (`netstat -Lan` showed a full accept queue such as `3/3/128`) while
  the daemon logged accepts only on loopback. The firewall was holding the
  connections for the unapproved ad-hoc-signed binary. `UserNotificationCenter`
  running is a hint that a dialog is pending.
- **Readiness is proven only at startup.** If the startup health proof of the
  tailnet listener fails (for example, during the firewall hold), the address
  stays out of `ready_addresses` until the DEV restarts, even after the
  firewall allows it.
- **Prebuilt restart needs the build checkout.** `./build.sh dev-prebuilt`
  refuses unless the checkout HEAD equals the recorded build; a different
  worktree is refused too. Rebuild from the DEV's own checkout instead.
- **Eval-only merges break restart proof.** A merge after the DEV build made
  the launcher refuse `dev-prebuilt`, and the fallback accepted the running
  daemon, so a whole endurance run "passed" with zero real restarts. Freeze the
  DEV checkout for a run, or rebuild on the final HEAD first.
- **Agent pads are ephemeral.** A tester's pad is deleted when its tab closes.
  Keep harness code in the repo (`evals/...`), not in pads, and never import one
  tester's pad from another.
- **Different API ports defeat discovery.** UDP beacons go to the peer's IP
  on the SENDER's own API port, and each DEV listens only on its own port. A Mac
  DEV on 7697 and a Windows DEV on 7686 never discover each other ("No
  discovered peers", "Peer not found in discovered list"), although HTTP works
  both ways. Pair by explicit address instead (`mesh connect --addr`).
- **Never PROD.** Production refuses `mesh_tailnet_host`; do not try to work
  around that, and never pair a PROD identity into the test mesh.
