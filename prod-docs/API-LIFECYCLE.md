# Application lifecycle API

Use the packaged CLI for cold install/update shutdown:

```bash
domios app quit --exact --json
```

This is local-only. It uses the selected instance's ordinary local credentials without putting a token in argv. Remote Mesh callers and service-push credentials cannot request application shutdown. No automatic retry occurs.

`GET /api/app/quit/snapshot` returns the exact server readiness record and its currently authenticated native client connection/process, or `client: null` when no native client is attached. `POST /api/app/quit` accepts that complete snapshot as deny-unknown-fields JSON. Both routes require ordinary API authentication and local caller admission. Transport bodies are bounded to32KiB; the exact snapshot is limited to8KiB before dispatch. A stale build, identity, process creation token, server epoch or client generation refuses before dispatch.

The CLI pins the selected server and native client before POST. The attached client enters the same exact Quit gate used by native Quit. The server sends successful protected completion only after required cleanup/persistence succeeds. A headless server uses the same cleanup with no fabricated client. The CLI then observes exit through its retained process handles. HTTP health, an admission ACK, a window disappearing, or process disappearance without cleanup evidence is insufficient.

Exit0 prints exactly one JSON value with `outcome: "confirmed"` and `receipt`. The receipt contains schema1, bootstrap_protocol2, api_version, server_build, instance_id, resolved identity, server_epoch, shutdown_completion (`operation: "shutdown"`, `cleanup: "completed"`), and server/client process records (`pid`, exact OS `created` token, executable) with `exit: "observed"`. A headless receipt has client:null. The receipt contains no token, challenge or raw bootstrap reply.

Nonzero outcomes are `refused` with reason busy, unsaved_work, no_instance, mismatched_build, unavailable or identity_mismatch; or `uncertain` with reason deadline, disconnected, invalid_completion or exit_unobserved. Neither authorizes replacing files. Busy means an existing Quit remains admitted; uncertainty retains admission and never triggers redispatch. Refusal reasons are finite; a platform without an unsaved-work producer does not invent such evidence.

Installers must invoke their trusted packaged CLI, validate its complete confirmed receipt against the selected instance, and replace files only after exit0. A copied JSON receipt is not a reusable authorization token. This source contract does not itself prove a native platform, packaged installation, or updater run.


## Additional REST routes

Account fields: ACCOUNT. Backup and update contracts: API-STATE. Listener and web-serving behavior: MESH.

| Method | Path | Operation |
|--------|------|-----------|
| GET | `/api/analytics/status` | Read analytics consent and queue status. |
| GET | `/api/app/quit/snapshot` | Read the exact Quit readiness snapshot. |
| GET | `/api/desktop/recovery` | Read desktop recovery state. |
| GET | `/api/essence-source` | Read essence source content. |
| GET | `/api/frontend/bootstrap` | Read frontend bootstrap state. |
| GET | `/api/frontend/clients/{id}/checkpoint` | Read a frontend client checkpoint. |
| GET | `/api/frontend/status` | Read interface bundle status. |
| GET | `/api/listener/status` | Read actual and configured listener state. |
| GET | `/api/network/serve/status` | Read web-serving status. |
| GET | `/api/session/backups/{name}/export` | Export a sanitized session backup. |
| GET | `/api/sessions/transcript` | Read a recorded session transcript. |
| GET | `/api/system/default-cwd` | Read the default shell working directory. |
| GET | `/api/system/shell-diagnostics` | Read host shell diagnostics. |
| GET | `/api/themes/location` | Read the user theme directory. |
| GET | `/api/update/availability` | Read combined app, plugin and interface update availability. |
| GET | `/frontend/bootstrap.js` | Read the frontend bootstrap observer script. |
| GET | `/frontend/{release_id}/{*path}` | Read a versioned frontend asset. |
| GET | `/manifest.webmanifest` | Read the web app manifest. |
| PATCH | `/api/crons/{id}` | Patch cron settings (local owner). |
| POST | `/api/account/billing` | Run an account billing operation. |
| POST | `/api/account/callback` | Complete account sign-in with the verified callback. |
| POST | `/api/account/neighbors` | Manage account Neighbor channels. |
| POST | `/api/account/neighbors/runtime` | Manage the local Neighbor receiving runtime. |
| POST | `/api/analytics/consent` | Set analytics consent. |
| POST | `/api/analytics/event` | Record an admitted typed analytics event. |
| POST | `/api/app/quit` | Submit the exact Quit snapshot. |
| POST | `/api/app/restart` | Request a local application restart. |
| POST | `/api/attachments/clipboard-file` | Save clipboard file bytes as an attachment. |
| POST | `/api/attachments/clipboard-image` | Save clipboard image bytes as an attachment. |
| POST | `/api/attachments/path-kinds` | Classify attachment paths. |
| POST | `/api/attachments/upload` | Upload attachment bytes. |
| POST | `/api/clipboard/copy-image` | Copy an image to the host clipboard. |
| POST | `/api/diagnostics/webgl` | Record a bounded WebGL diagnostic. |
| POST | `/api/essence-maps` | List essence maps for a project. |
| POST | `/api/frontend/activate` | Activate a verified interface bundle. |
| POST | `/api/frontend/check` | Check for a compatible interface bundle. |
| POST | `/api/frontend/diagnostic` | Admit a bounded frontend diagnostic. |
| POST | `/api/frontend/healthy` | Confirm interface health after activation. |
| POST | `/api/frontend/ready` | Record frontend readiness. |
| POST | `/api/frontend/rollback` | Roll back interface activation. |
| POST | `/api/listener/restart` | Request a local listener restart. |
| POST | `/api/model-providers` | Manage decision model-provider connections. |
| POST | `/api/network/serve/disable` | Disable web serving. |
| POST | `/api/network/serve/enable` | Enable web serving. |
| POST | `/api/session/backups/import` | Import an exchanged session backup. |
| POST | `/api/setup/hooks/restore` | Restore saved integration hooks. |
| POST | `/api/system/install-pwsh` | Open a PowerShell installer tab (Windows only). |
| POST | `/manifest-bootstrap` | Establish the web manifest bootstrap credential. |
| PUT | `/api/frontend/clients/{id}/checkpoint` | Save a frontend client checkpoint. |

## Provider lifecycle ingress

Provider hook bodies and nonce admission: API-TABS.

| Method | Path | Operation |
|--------|------|-----------|
| POST | `/api/tabs/{id}/hook/context` | Admit the bounded provider context feed. |
| POST | `/api/tabs/{id}/hook/permission` | Admit a provider permission hook. |
| POST | `/api/tabs/{id}/hook/provider_event` | Admit a provider-defined lifecycle event. |
| POST | `/api/tabs/{id}/hook/session_start` | Admit a provider session-start hook. |
| POST | `/api/tabs/{id}/hook/stop` | Admit a provider stop hook. |
| POST | `/api/tabs/{id}/launch-artifacts/release` | Release owned provider launch artifacts. |

<!-- revision: 3 -->
