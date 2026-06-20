# Plugin abstraction / data-flow review — opportunities (5 reviewers, for Andrey)

> Read-only architecture review. NOT bugs — abstraction/DRY/hardcode/elegance opportunities.
> Synthesize into a refactor plan AFTER all 5 report; Andrey picks what to do.

## Facet 2 — Host API + JS runtime (codex) — DONE
HIGH-value:
- A2.1 (M) HostBinding REGISTRY/table {name, permission gate, arg codec, handler, preamble}
  instead of the flat host_api.rs fn list — one uniform permission/error/logging shape + SDK gen.
- A2.2 (M/L) Generic PluginJobRegistry<TState,TPoll> — exec / fetch-stream / worker-agent each
  re-implement job-id start/poll/close/drop/limits/not-found. Collapse to one + adapters.
- A2.3 (M) Typed JSON bridge: bind_json auto-serializes Result<T,HostError>; preamble + SDK
  generated/conformance-tested from one metadata source (kills stringly names + SDK drift).
- A2.6 (M) Central HostPermission gate + manifest PermissionsConfig (gating scattered, denial
  shapes inconsistent, mem/env ungated). Security review becomes data-driven.
MED/lower:
- A2.4 (S/M) describeModelSwitch SDK/FE-vs-runtime DRIFT — FE calls it but core route + SDK lack
  it; make it a real capability or remove. [actionable now]
- A2.5 (S/M) host.worker/agent default backend hardcodes "claude" -> resolve_default_agent_backend
  (capability/config). [dup of P1b LOW]
- A2.7 (S/M) HostEndpoint via url::Url (host_of/can_reach string-split; fragile IPv6/creds/ports).
- A2.8 (M) ViewInvocationContext built host-side (cwd is stamped in FE PluginView today).
- A2.9 (S/M) PluginMethod<TArgs,TOut> optional-capability descriptor (stringly "not a function").
- A2.10 (M) Author-facing typed host.process/http only; raw JSON-string calls leak + can hold VM lock.

## Facet 4 — Session / model / detection (codex) — DONE
HIGH-value (big DRY):
- A4.1 (M) host.jsonl.mapLines + host.chat block/text helpers — EVERY JSONL provider re-implements
  the line loop + corruption tolerance + timestamp + text-block extraction (100+ lines each,
  "duplicated by design" comment). Providers keep only schema-specific mappers.
- A4.2 (L) Declarative manifest.sessions SessionStore {kind jsonl|sqlite|custom, roots+glob+depth,
  cwd encoding, id-extraction regex, existence files, metadata sources, model selector} — providers
  hand-walk the filesystem because the manifest can't describe recursive/pattern stores (Codex
  rollout tree, Cursor transcript tree). Make detection manifest-first.
- A4.8 (M) resolveNewSession({cwd,exclude,launchedAfter,maxAge}) -> {id, confidence} host-side
  unique-candidate policy (Cursor/Codex/Claude each reimplement the refuse-ambiguous rule; README
  contract ("newest in cwd") mismatches the real identity-bound rule).
MED:
- A4.3 (M) detectSessionModelRaw {rawId,source} + central catalog alias/family/display (detection
  mixes extraction + normalization; each provider emits raw/short/family inconsistently).
- A4.4 (M) Claude session logic duplicated native (agent_claude/provider/sessions.rs) vs JS
  (provider-claude/src/session.ts) — one source of truth.
- A4.5 (M/L) ONE canonical provider chat IR — JSONL providers emit RichMessage (Rust flattens),
  structured (opencode) returns ChatMsg directly knowing the FE shape. Unify on RichMessage.
- A4.6 (S/M) supports_chat_ui derived from the SessionStore capability, not JS-method presence
  (manifest-only JSONL providers wrongly miss chat UI).
- A4.7 (M) model_catalog keep {seed, live, override} separate; discoverModels returns live-only;
  drop the per-provider `discovery || manifestModels` fallback (catalog already has precedence).
  [RELATED to the in-flight DM1 dynamic-models work — align it.]
- A4.9 (S/M) Central search-SessionMessage derived from RichMessage (providers implement BOTH a
  text-search parser AND a rich-chat parser).

## Pending: absrev1 (provider/capability), absrev3 (transport/spawn/lifecycle), absrev5 (chat/messaging/toolcall)

## Facet 3 — Transport / spawn / lifecycle (opus) — DONE — HIGHEST VALUE
Two seams both only half-wired: AgentTransport (spawn/launch) + PaneTransport (I/O).
LIVE-BUG-ish:
- A3.1 (HIGH, leak+dup, maybe live bug) PaneTransport WRITE-seam bypassed: send_text/send_text_batch/
  resize/kill hand-branch on pane_chat_backing instead of pane_transport().send(). The bite:
  ChatBackendTransport::send gates outbound stages on pane_has_agent (the M3 fix: plain-shell
  chatBackend must NOT get <context>), but the ACTUALLY-USED path chatbackend_pane_send +
  send_text_batch chat branch run run_outbound_stages UNCONDITIONALLY -> shell chatBackend gets
  context it shouldn't. Fix: route all writes through pane_transport().send(); delete the dup paths;
  extend the CI grep guard to writes.
- A3.2 (HIGH, dup) Two PTY-agent launch pipelines: atomic PtyTransport::launch returns an
  FE-typed UNVERIFIED string (no task/session-id/boot-gate/verify); the real one is
  launch_agent_in_background (backend, verified). Atomic only "works" because the FE button carries
  no task. THIS IS WHERE ROG's Windows bug lives. Fix: atomic delegates to spawn_pane_with_ready/
  launch_agent_in_background (launch_command=None, backend delivers); kill the FE-typed path.
- A3.3 (HIGH, name collision) TWO structs named PtyTransport (AgentTransport spawn vs PaneTransport
  I/O) + ChatTransport vs ChatBackendTransport. Rename spawn-side -> PtyLaunch/ChatLaunch. Cheap.
DRY:
- A3.4 (S) CODETERM_* env map rebuilt 4-5x; pane_env() canonical unused (copies even drift).
- A3.5 (S) workspace-resolution block copy-pasted 7x; only transport.rs uses resolve_spawn_workspace.
- A3.6 (S) chatbackend connecting-backing stamp byte-identical in ChatTransport::launch + spawn_chatbackend_impl.
- A3.7 (M) create-tab + apply-metadata sequence repeated across spawn impls -> PaneMetadata + create_tab_with.
- A3.8 (S) transport_of matched twice; atomic None|Pty arms identical -> collapse + shared classify().
- A3.9 (M) spawn_visible_agent_impl vs orchestrator::spawn_agent ~80% duplicate.
- A3.10 (S) pane_chat_backing/pane_has_agent take mux.write() for a pure read.
BUNDLE: A3.1+A3.2+A3.3 collapse the two-seam/two-pipeline split — one send dispatch, one launcher,
removes the unverified FE-typed launch, fixes the M3 gate divergence. (A3.2 subsumes ROG's hotfix.)

## Pending: absrev1 (provider/capability), absrev5 (chat/messaging/toolcall)

## Facet 1 — Provider / capability / descriptor (opus) — DONE
- A1.1 (S,safe) describe() hand-duplicated in BOTH traits (21 fields, no Default derive) -> derive Default + ..Default::default().
- A1.2 (S) system_prompt_delivery = DEAD weight (zero branching consumers; CliFlag built nowhere) -> delete.
- A1.3 (M, elegant) behavior_config + presets + system_prompt_delivery = 3 parallel fields for one concept -> one enum BehaviorModel{None,OwnPrompts,Presets{presets,delivery},Graph} (illegal states unrepresentable; presets has ZERO readers today).
- A1.4 (S,safe) trait-default bodies copy-pasted into JsProvider (build_prompt_response/has_event_markers/model_launch_args) -> shared free fns (kills native/JS drift).
- A1.5 (M) JsProvider has_js/native fallback boilerplate x7 -> js_or_native helper/macro.
- A1.6 (L; safe slice) Pty/Chat trait duality duplicates the IDENTITY surface -> extract ProviderIdentity super-trait (defer the single-trait+enum collapse).
- A1.7 (S) supports_chat_ui redundant w/ transport for chat backends.
- core is CLEAN of provider-id hardcode in spawn/detection (all transport-driven); only literal claude left = general_agent/instructions.rs render_agent_md (instructions facet).

## Facet 5 — Chat / messaging / tool-call (opus) — DONE
- A5.1 (M) THREE duplicated poll engines (tailer JSONL-offset / structured sqlite / chatbackend plugin) reimplement the same stop/poll/dedup/emit/sleep loop; only chatbackend has adaptive backoff -> generic PollEngine<PollSource> + pluggable dedup (JSONL+sqlite inherit idle backoff; today they busy-poll 100ms per open pane).
- A5.2 (S,SAFE DELETE) dead raw-text tailer path (spawn_raw_tailer/RawTextSink/raw_text_sink.rs) — zero callers, no FE listener. Delete ~100 LoC.
- A5.3 (M) sender identity has TWO encodings: PTY inline marker in ChatMsg.content (FE re-parses at render) vs chatBackend structured from_* fields -> parse the marker ONCE server-side into the SAME structured fields; deletes ~80 LoC FE marker re-parsing.
- A5.4 (S,SAFE DELETE) marker convenience constructors largely dead + triplicated Rust/TS (pane/report/mesh/photo/attachment unused) -> delete, keep parseMarker + live builders.
- A5.5 (S fixture) ChatMsg (Rust) <-> NormalizedChatMessage (TS) hand-maintained twins with NO parity fixture (the load-bearing wire type) -> add serde-roundtrip parity fixture (or ts-rs codegen).
- A5.6 (S) MessageKind::marker_kind()==inbound_kind() byte-identical -> one as_str(); TS union derived.
- A5.7 (S,SAFE DELETE) dead FE ProviderChatAdapter interface + stale comment -> delete.
- CLEAN: rich_to_chat_msgs single flattener; toolcall_parse native+JS boundary; FE has no per-provider parser; FlattenDeltaSink unifies JSONL+sqlite render.

## ════ SYNTHESIS (cross-cutting themes, ranked for Andrey) ════
THEME A — per-provider DRY (BIGGEST simplification): A4.1 shared JSONL helpers, A4.2 declarative
  SessionStore manifest, A4.8 host-side unique-session resolution, A4.5 one canonical chat IR,
  A4.9 search-from-RichMessage, A5.1 one poll engine, A1.4 shared trait-default bodies.
THEME B — transport seam unification (HIGH value, has LIVE bugs): A3.1 route writes through
  PaneTransport (fixes M3 <context> gate divergence), A3.2 unify launch via launch_agent_in_background
  (the Windows-launch-bug seam; kill FE-typed-unverified), A3.3 rename colliding PtyTransport structs,
  + A3.4-3.10 spawn DRY (env map, workspace-resolution x7, create-tab-metadata, etc).
THEME C — host API coherence: A2.1 HostBinding registry, A2.2 generic PluginJobRegistry, A2.3 typed
  JSON bridge, A2.6 central permission gate, A2.8 host-side view context, A2.9 optional-method dispatch.
THEME D — descriptor/capability cleanup: A1.1 Default, A1.3 BehaviorModel enum, A1.6 ProviderIdentity.
THEME E — hardcode/drift removal: A2.5 claude default backend, A2.4 describeModelSwitch drift,
  A4.7 model_catalog seed/live/override (align w/ in-flight DM1), A4.4 claude native/JS dedup, A5.5 chat-type parity.
THEME F — SAFE DELETIONS (quick wins, provably-dead): A5.2 raw tailer, A5.4 marker ctors, A5.7
  ProviderChatAdapter, A1.2 system_prompt_delivery.

RECOMMENDED SEQUENCE:
 1. Wave Q (S, safe): Theme F deletions + A1.1 Default + A1.4 shared bodies + A3.3 rename + A2.5 +
    A3.10 mux.read + A5.6 — pure cleanup/drift removal, ~zero risk.
 2. Wave T (M/L): Theme B transport-seam unification — AFTER ROG's Windows-launch hotfix lands
    (A3.2 subsumes it). Fixes the M3 gate live bug + collapses the two-pipeline split.
 3. Wave H (M): Theme C host-API coherence (registry + job-poll + typed bridge + perms).
 4. Wave P (M/L): Theme A per-provider DRY (host JSONL/chat helpers + declarative SessionStore +
    unique-session resolution + one poll engine) — the biggest LoC reduction + flexibility win.
 5. Wave D (M): Theme D/E descriptor + IR + catalog cleanup + parity guards.
