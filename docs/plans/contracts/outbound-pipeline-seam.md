# Contract — outbound message pipeline + transport seam

> **Shared by Track B and Track C** (see `docs/plans/2026-06-17-plugins-rethink-implementation.md`,
> §1.6 R1). This is the single load-bearing boundary the two host tracks share.
> **No code here — exact signatures only.** Where this conflicts with looser prose
> in a Track, **this wins** (it is the §1.6 R1 resolution made concrete).

## Why this exists

Two host tracks both touch the send path:

- **Track B** owns every **send/output entry** edit and the **transport dispatch**
  (PTY vs `chatBackend`). It ships the pipeline hook as an **identity function** so
  it compiles and the send path works before any stage exists.
- **Track C** owns the **stage pipeline** (`chatpipeline` module): the
  `chatPreprocessor` stage, the transcriber recast as a compose-time stage, the
  conflict-filter check. C **fills the hook body**; it does **not** edit B's entry
  files.

The only line both tracks need to agree on is the hook call. B authors it once; C
fills the module it delegates to. Conflict-free by construction.

## Ownership map (who edits what)

| Surface | Owner | Note |
|---|---|---|
| Send entry sites (`api/pane_routes.rs:204`, dispatch send cmd, `/api/pty/{id}/write`) | **B** | B inserts the hook call; C never edits these files |
| Transport dispatch (PTY vs `chatBackend`) | **B** | branch on `Pane.backing` (F1) |
| `run_outbound_stages` declaration + identity impl | **B** | ships as `|_, t| t` |
| `chatpipeline` module (stages, ordering, conflict filter) | **C** | fills the hook body |
| `chatPreprocessor` stage registry + transcriber-as-stage | **C** | |

## 1. Stage-pipeline hook signature

The single hook B exposes and C fills. **B ships it as identity; C replaces the
body.** B calls it in the send seam **after** user-turn gating and **before**
dispatching to transport.

```
// Rust, host side. Owned by B as a thin pass-through; body implemented by C.
fn run_outbound_stages(pane_id: &PaneId, text: String) -> String
//   B identity impl:  |_, t| t
//   C impl: run the ordered stages for this pane, each stage may rewrite `text`.
```

Rules:

- **Pure-ish, text-in/text-out.** A stage receives the current `text` and returns
  the (possibly rewritten) `text`. It never sends — it only transforms.
- **Synchronous from the call site.** If a stage needs external I/O it uses the
  host job-id/poll bridge internally (F4); the seam call itself returns text.
- **User-turn only.** B gates the call on user-turn detection (`source_pane` /
  `svc_token_match` — service-push = skip). System senders are excluded
  (orchestrator task delivery, agent restart, hierarchy reports, GA init,
  post-update, install suggest — see §1.5 F3 for the exact list). C assumes the
  hook is only reached for user turns.
- **Both web and native entries route through it.** The browser composer path
  (`web_routes.rs:316` `/api/pty/{id}/write`) and the native/REST `send_text`
  (`pane_routes.rs:204`) both reach the hook (F3) — B owns wiring both.

## 2. Transport-dispatch signature

After stages run, B dispatches by the pane's backing discriminator (F1). C does
**not** touch this.

```
// Branch on Pane.backing  (Shell | Agent | ChatBackend)
match pane.backing {
    Shell | Agent  => /* existing PTY write path (ensure_pane_pty_for_send → PTY) */,
    ChatBackend    => chatbackend_registry.get(plugin_id)?.send_message(session_id, text),
}
```

- **PTY panes:** unchanged. `Shell`/`Agent` keep the existing
  `ensure_pane_pty_for_send` → PTY write path. The PTY provider path is untouched
  (additive-only discipline).
- **`ChatBackend` panes:** route to the registered provider's `sendMessage`; never
  auto-spawn a PTY (the F1 [RESTORE GAP] / auto-PTY-spawn early-return).
- **Output side (mirror):** `get_pane_output` returns the PTY text for PTY panes
  and the **flattened transcript** for `ChatBackend` panes. The `get-output`
  contract is **unchanged** (R6): `{ output: <plain text>, offset }`. A
  `ChatBackend` pane flattens its transcript to the same plain text + offset
  (e.g. `"[user] …\n[assistant] …\n"`).

## 3. Ordering rules

The pipeline is **ordered by slot**, compose-time stages before send-time stages,
then transport:

```
USER text
  → [compose-time stages]   match: "voice"   (transcriber → text)
  → [send-time stages]      match: "text"    (chatPreprocessor → edited text)
  → transport               (PTY | chatBackend.sendMessage)
```

- **Compose-time (`match: "voice"`)** run first — they turn a non-text attachment
  into text (the transcriber recast, Track C).
- **Send-time (`match: "text"`)** run next — they edit the text in place
  (`chatPreprocessor`, returns `{ text }` or `null` for passthrough).
- Within a slot, stages run in **registration order** (deterministic; the host
  controls registration). A `null` return = passthrough (text unchanged).
- Transport dispatch (§2) is the terminal step and is **not** a stage.

## 4. Conflict match-filter shape

Each stage declares the **slot** it grabs via `match`. **Two stages claiming the
same slot is a conflict, flagged at install time** (not silently chained).

- `chatPreprocessor.match` is the literal `"text"` (see the SDK `ChatPreprocessor`
  type). The transcriber stage's slot is `"voice"`.
- The conflict check is a set over installed stages keyed by `match`: a second
  stage registering an already-claimed slot is **rejected at install** with a
  conflict error naming both plugin ids. (Track C owns this check; the SDK type
  carries the `match` discriminant that makes it enforceable.)
- This guards the seam from two plugins silently fighting over the same outbound
  slot — the project's capability-not-identity discipline applied to stages.

## Integration order (so B and C never collide)

1. **B lands first:** send entries + transport dispatch + the identity
   `run_outbound_stages`. The send path works end-to-end with no stages.
2. **C rebases on B**, implements the `chatpipeline` module, and fills the hook
   body. C's `host.*` work (host_api, CLI) is independent of B and proceeds in
   parallel from day one.

<!-- revision: 1 -->
