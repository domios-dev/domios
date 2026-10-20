# Application lifecycle API

Use the packaged CLI for cold install/update shutdown:

```bash
codeterm app quit --exact --json
```

This is local-only. It uses the selected instance's ordinary local credentials without putting a token in argv. Remote Mesh callers and service-push credentials cannot request application shutdown. No automatic retry occurs.

`GET /api/app/quit/snapshot` returns the exact server readiness record and its currently authenticated native client connection/process, or `client: null` when no native client is attached. `POST /api/app/quit` accepts that complete snapshot as deny-unknown-fields JSON. Both routes require ordinary API authentication and local caller admission. Transport bodies are bounded to32KiB; the exact snapshot is limited to8KiB before dispatch. A stale build, identity, process creation token, server epoch or client generation refuses before dispatch.

The CLI pins the selected server and native client before POST. The attached client enters the same exact Quit gate used by native Quit. The server sends successful protected completion only after required cleanup/persistence succeeds. A headless server uses the same cleanup with no fabricated client. The CLI then observes exit through its retained process handles. HTTP health, an admission ACK, a window disappearing, or process disappearance without cleanup evidence is insufficient.

Exit0 prints exactly one JSON value with `outcome: "confirmed"` and `receipt`. The receipt contains schema1, bootstrap_protocol2, api_version, server_build, instance_id, resolved identity, server_epoch, shutdown_completion (`operation: "shutdown"`, `cleanup: "completed"`), and server/client process records (`pid`, exact OS `created` token, executable) with `exit: "observed"`. A headless receipt has client:null. The receipt contains no token, challenge or raw bootstrap reply.

Nonzero outcomes are `refused` with reason busy, unsaved_work, no_instance, mismatched_build, unavailable or identity_mismatch; or `uncertain` with reason deadline, disconnected, invalid_completion or exit_unobserved. Neither authorizes replacing files. Busy means an existing Quit remains admitted; uncertainty retains admission and never triggers redispatch. Refusal reasons are finite; a platform without an unsaved-work producer does not invent such evidence.

Installers must invoke their trusted packaged CLI, validate its complete confirmed receipt against the selected instance, and replace files only after exit0. A copied JSON receipt is not a reusable authorization token. This source contract does not itself prove a native platform, packaged installation, or updater run.

<!-- revision: 1 -->
