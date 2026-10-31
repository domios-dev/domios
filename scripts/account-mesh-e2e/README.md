# Account Mesh real-machine rollout harness

`scripts/account-mesh-e2e.mjs` checks the Account Mesh rollout against real installations: two existing paired machines (`paired_a`, `paired_b`), a separate unlinked Add fixture (`add_a`, `add_b`) and a third Free machine (`third`). The expected outcomes come from the targets themselves: their real pair records, mesh put/get round trips, server machine counts and Browser text. The harness keeps no record of its own that it compares against. A source pass is never real-machine evidence. Only Turner accepts runtime evidence and runs the release gate.

## Commands

```sh
node scripts/account-mesh-e2e.mjs --manifest ABS_MANIFEST.json --out ABS_EVIDENCE_DIR --preflight-only
node scripts/account-mesh-e2e.mjs --manifest ABS_MANIFEST.json --out ABS_EVIDENCE_DIR
node scripts/account-mesh-e2e.mjs --manifest ABS_MANIFEST.json --out ABS_EVIDENCE_DIR --phase PHASE_ID
```

| Exit | Classification | Meaning |
|---|---|---|
| 0 | `preflight-pass-runtime-not-run` / `real-machine-pass-awaiting-turner-acceptance` | Preflight is clean, or every phase ran and every check passed |
| 1 | `runtime-fail` | At least one oracle failed; read the reasons and artifacts |
| 2 | `refused-before-effects` | Bad arguments, schema, target identity, build SHA, topology or evidence directory |
| 4 | `source-ready-runtime-held` | A phase is missing a named receipt, or only one phase was run |

## Refusals before effects

- Schema: unknown fields, missing fields and malformed SHAs, ports or uuids are refused.
- Ports: API port 7685 is always refused, along with every port and owner instance id under `forbidden`. A fixture is an isolated named instance. A local target on a non-default port must name its `instance`, because the CLI refuses to guess a socket.
- Distinct machines and accounts: `paired_a`, `paired_b` and `third` must be three different machines. The Add pair must be two different machines. The `primary`, `foreign` and `add` accounts must be three distinct Free accounts. A foreign sign-in on `paired_a` takes a durable foreign slot, so the Add fixture cannot share that account.
- Operations: argv may not contain `mesh disconnect|connect|approve|reject|unrevoke|broadcast`, `update install`, `app quit`, `config set`, `--mesh` or host-clock tools. `time_control` must declare `isolated_clock: true`.
- Preflight runs read-only, per target:
  - `identify`: port and instance.
  - `mesh status`: the exact `instance_id`, `api_port` and the daemon build against the old or new release SHA.
  - `cli-manifest`: built-in and product operation commands exist.
  - `account status` and `mesh peers`.
  - Topology: `paired_a` and `paired_b` are already paired, and the Add fixture and `third` are unlinked.
- Evidence directory: `--out` must equal `evidence_dir`, and it must be empty or created by the same manifest digest. A new run never mixes with old evidence.

## Manifest

Schema `domios.account-mesh-e2e.manifest/1`. The fields below define the accepted manifest.

- `release`: `client_sha`/`client_version`, `old_client_sha`/`old_client_version`, and the exact `backend_sha`/`backend_project`. The deployed backend SHA readback must equal `backend_sha`.
- `accounts.{primary,foreign,add}`: `ref` (a local-store reference, never a secret), `uid_sha256` and `plan: "free"`.
- `targets.ID`: `machine`, `platform`, `transport` (`local`, or `prefix` with a Turner-owned argv that reaches that machine), absolute `cli`, `api_port`, optional `instance`, `instance_id`, `account`, and `start` (`old` or `new`).
- `operations.KEY`: `argv`, `receipt` (the accepted server or client receipt the argv comes from) and `fields`, which maps each required logical field to a dotted JSON path in the operation's `--json` output.
  - Product operations run through the target CLI.
  - Hooks run a Turner-owned absolute command.
  - Placeholders: `{target} {machine} {instance_id} {peer_instance_id} {peer_installation_id} {account_ref} {now_ms} {client_sha} {run_id}`.
- `authorizations.PHASE`: absolute `receipt` path, its `sha256` and `granted_by`. A missing or changed receipt holds that phase.
- `deadlines`: `command_ms ≤ 60000`, `phase_ms ≤ 900000`, `run_ms ≤ 3600000`. Deadlines bound failure only. Completion is always the command's own exit or response. There are no sleeps and no polling.

### Operation seam (fields the harness reads)

| Key | Kind | Fields | Supplier |
|---|---|---|---|
| `enroll` | product | `ok error_code limit_key limit_value operation_id` | task_0rimhx enrollment DTO |
| `membership` | product | `state expires_at_ms installation_id` | task_0rimhx membership state |
| `machines` | product | `installation_ids slots_used slots_limit` | task_u32oh0 callable list, via the client |
| `add` | product | `ok error_code` | task_0ka533 / task_s0j8cr Add |
| `revoke` | product | `ok` | task_0rimhx revoke |
| `endpoint` | product | `present` | task_u32oh0 endpoint read, via the client |
| `signout_cleanup` | product | `cleanup` (`confirmed` or `pending`) | task_0rimhx sign-out lifecycle |
| `refusal_events` | product | `operation_ids` | task_0rimhx structured refusal event |
| `stream_probe` | product | `delivered` | task_s0j8cr stream admission |
| `login` | hook | — | Turner real sign-in flow; harness waits on its exit |
| `upgrade` / `restart` | hook | — (build read back via `mesh status`) | Turner native fixture lifecycle |
| `registry_block` / `registry_unblock` | hook | — | Turner isolated network drill |
| `firebase_block` / `firebase_unblock` | hook | — | Turner isolated network drill, direct P2P kept |
| `firebase_packets` / `discovery_packets` | hook | `connections` / `broadcast_packets` | observed packet capture, not static claims |
| `time_control` | hook | `now_ms` (`isolated_clock: true`) | reviewed isolated time drill; never the host clock |
| `firestore_probe` | hook | `denied` | closed-rules client read probe |
| `backend_readback` | hook | `sha project` | deployed backend SHA readback |
| `ui_snapshot` / `ui_plans_modal` | hook | `text` / `open_count` | Browser plugin (`codeterm plugin browser text`/`snapshot`) |

The built-in commands are the shipped `identify`, `cli-manifest`, `mesh status|peers`, `account status|logout` and `paste put|get|rm`. Mesh traffic probes run `paste put --from-stdin --mesh PEER`, then `paste get ID --mesh PEER`, and compare body digests. A timeout never counts as a block. A `peer_not_paired` refusal counts as a removed pairing, never as a block.

## Phases and cleanup owners

| Phase | Oracle | Cleanup owner |
|---|---|---|
| `release-identity` | The deployed backend SHA and project equal the manifest | none (read-only) |
| `upgrade-continuity` | Old/old, then old/new (mixed), then restart, then new/new. The pair key, `paired_at` and access stay equal, with no new records or requests. Traffic flows both ways. Messages acknowledged before each upgrade can be read back | harness deletes its pastes; Turner owns binary rollback |
| `adopt-in-place` | Primary sign-in and enroll on both. Traffic flows after each side adopts. Membership is `adopted`, with no re-pair | Turner |
| `signout-block` | Logout keeps the records, cleanup is `confirmed`, traffic and stream are blocked both ways, and the exact copy is shown. A foreign account stays blocked. The same account resumes without re-pairing | harness restores the primary sign-in through `login`; Turner if that fails |
| `add-online-only` | The account lists the unlinked machine. Add is refused while `add_b` has no permit, and no record appears on either side. After unblocking, Add links both directions with direct traffic | harness unblocks the registry; Turner owns the new link |
| `backend-offline-expiry` | With Firebase blocked, 10 P2P round trips cause zero observed backend connections. The isolated clock at `expires_at - 1` passes and at `expires_at` blocks. Records are preserved | harness unblocks and returns the isolated clock to real time |
| `isolation` | Account listings are disjoint, the same-account endpoint is visible, the foreign endpoint is hidden, and Firestore is denied. An offline sign-out reports `pending` and still blocks. No broadcast command, discovered rows or broadcast packets | harness unblocks and restores the primary sign-in on `paired_b` |
| `third-refused-revoke` | `third` signs in, but enroll is refused with `mesh.machines` and limit 2. Sign-in stays, there is exactly one refusal event and one plans modal, slots stay at 2, and `third` has no link. Revoking `paired_b` removes it from the list, frees a slot and removes its endpoint. `third` then enrolls | Turner (revoke is destructive by design) |

The harness never unpairs and never runs `mesh disconnect`. A failed phase still runs its cleanup, and a cleanup failure is recorded with its owner.

## Operating recipe

1. Turner stages named instances on the real machines from the matching old and new binaries, plus three dedicated Free accounts. Credentials stay in each machine's local store; owner sessions and keys are never copied. Turner writes the manifest with exact SHAs and owner instance ids under `forbidden`.
2. Run `--preflight-only` and fix every refusal. Nothing has been mutated at this point.
3. Turner adds the operation argv from accepted sibling receipts, plus one authorization receipt per phase, then runs the full harness once on the final integration SHA. Held phases list their missing receipt.
4. Adverse proof: Turner disables one isolated membership or traffic gate on a fixture, then reruns only that case with `--phase ID` into the same evidence directory. The oracle must fail. Turner restores the gate, repeats that phase and records both results.
5. Evidence lands in `ABS_EVIDENCE_DIR`:
   - `run.json`: the manifest digest.
   - `events.jsonl`: an order journal.
   - `artifacts/NNNN-*.json`: each redacted command with argv, exit, digest and parsed JSON. Uids, emails, addresses and keys are hashed or dropped.
   - `results-*.json|md`.
