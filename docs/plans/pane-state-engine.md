# Real-time pane state engine + state corpus

> Owner: Dante, from Andrey 2026-06-20. The product goal: in REAL TIME, know what an agent
> pane is doing and show it beautifully — is it WORKING, IDLE (your turn), or asking a QUESTION;
> and if a question, WHICH kind. Stream the live message before it hits jsonl. Iterate + measure
> it on a growing labeled corpus of real agent behaviour. Branch `plugins-rethink`. Builds on
> P1a `SignalExtractor`, `PaneState`, the attributed-grid detector (W1), and `agent_state_replay`.

## The state model (Andrey's 3 + sub-classification)
Top level — exactly three:
- **WORKING** — the agent is producing output / thinking. (UI: tasteful "typing…" text, not a dot.)
- **IDLE** — turn complete, quiescent, ready prompt, no active picker. Your turn. (Distinct from
  today's `IdleStuck` = "owed a turn but parked".)
- **QUESTION** — quiescent AND blocked on user input (an interactive prompt/picker is active).

QUESTION sub-classifies (the "essence of when the model needs something"):
- **rate_limit** (tokens limited) · **auth** (login/OAuth/API key) · **permission** (approve this
  command/patch) · **model_choice** (AskUserQuestion / options the model offers) · **update**
  (CLI self-update prompt) · **error** (failed, needs ack) · **unknown** (a real prompt we can't
  classify yet — surfaced honestly as a generic question card, never hidden).

Mapping to what EXISTS: `PaneState::{Working, WaitingOnAuth→auth, RateLimited→rate_limit,
ClarifyingQuestion→model_choice, UpdatePrompt→update, Unknown}` already cover most subtypes.
GAPS to add: a clean **IDLE** (turn-complete-your-turn), **permission**, **error**, and the
3-level grouping over the flat enum.

## Tiered confidence (the probabilistic design)
For each signal, take the highest-confidence source available, fall back down:
1. **Tier 1 — provider handle** (high confidence, where it exists in PTY mode). opencode SSE
   (proven, W3): message events → WORKING/IDLE; `permission.asked`→permission, `question.asked`→
   model_choice — classification for free, no scraping. (codex/cursor only if we add JSON-RPC/ACP
   transports later.)
2. **Tier 2 — console: SignalExtractor + attributed grid** (the bulletproof baseline, all
   providers). WORKING/IDLE from two-clock quiescence (`byte_idle`/`semantic_idle` + `Motion`);
   QUESTION from `picker_active` + the W1 attributed-grid fusion detector; sub-class via STRUCTURAL
   fingerprints (footer-hint lexicon w/ synonyms, box/region shape, URL-for-auth, usage-for-limit)
   — never literal vendor strings, multi-lingual aware.
3. **Fallback — Unknown** (honest). A prompt we detect but can't classify still surfaces as a
   generic card.

## Real-time last-message streaming (read terminal before jsonl)
Today the transcript is parsed from jsonl AFTER the turn lands. Goal: render the CURRENT assistant
message live, then swap seamlessly when jsonl finalizes.
- Source: `SignalExtractor.delta.appended_text` ALREADY yields the newly-streamed text per step —
  that is the live message delta. Accumulate it as the in-progress assistant bubble while state ==
  WORKING; bound it to the assistant region (attributed grid) so chrome/prior turns are excluded.
- Swap: when the jsonl `RichMessage` for that turn arrives, replace the live bubble with the
  canonical parsed message, deduping by content (reuse the `matchedOptimisticEchoes` / content-skew
  logic) so the swap is invisible.
- Distinguish messages: tie the live stream to the active turn via the SignalExtractor stream +
  the message `seq` (the ordering fix already added a source-ordinal).

## Typing indicator (UI)
Drive off WORKING. Replace the dot in `ChatComposer.tsx` with a tasteful inline "typing…" (or a
subtle animated word) in the chat flow — not inside a bubble. Frontend-design pass.

## The state corpus — is it worth it? (Andrey's question)
**Verdict: worth it, but SCOPED — don't hand-build 200 perfect dumps.** Two distinct needs:
- **State (working/idle/question) needs TIME-SERIES** `.cast` (dynamic motion over time), NOT
  static screens — `agent_state_replay` already consumes exactly this. Static-frame snapshots only
  serve picker PARSING.
- Two tiers of corpus:
  - **Curated GATING set** (~5-10 per state per provider): hand-verified `.cast` with a labeled
    state timeline. Small, gates CI, must stay green.
  - **Generated ACCURACY/DRIFT set** (~40-50 per provider): auto-produced, measured (not gated),
    grows over time.
- **How to generate cheaply** (yes, realistic + feasible):
  1. **Mine our 692 real sessions** (`~/.claude/projects`, `~/.codex/sessions`, cursor) for
     representative prompt SEQUENCES (what real tasks looked like) → replay those prompts to
     re-drive a live agent and record its terminal `.cast`. Real scenarios, for free.
  2. **Scenario templates**: a few fake-project skeletons + task prompts ("write a function", "fix
     this bug", "run the tests") that exercise each state — make it WORK (code), finish (IDLE),
     `/model` (model_choice), trigger an approval (permission), spawn logged-out (auth).
  3. **Auto-label**: the generator KNOWS what it triggered ("I sent /model at t=X → expect
     question:model_choice from t=X") → emits the ground-truth timeline automatically; human
     spot-checks. This is capture→propose-expected applied to STATE.
  - Run as an OFFLINE BATCH generator (slow, tokens, dev load) — NOT in CI. CI replays the
    recorded corpus offline (fast, deterministic via `agent_state_replay`).
- Hard-to-force states (rate_limit) — capture opportunistically when they happen in real use +
  one synthetic fixture; don't block on forcing them.

## Metrics (what Andrey wants to measure)
Over the labeled corpus, via `agent_state_replay`:
- per-state precision/recall; the bench already counts `falseWaiting`/`falseTurnComplete` (false
  IDLE/turn-complete during streaming) — extend to false-WORKING + question-subtype confusion.
- transition latency: how fast WORKING→IDLE and →QUESTION are detected after the real transition.
- a scorecard per provider × version (drift over agent releases).

## Phasing
- **Phase A — state engine (Tier 2)**: wire `SignalExtractor` into the live status path (the
  never-done P2) + a 3-level classifier (WORKING/IDLE/QUESTION) over two-clock quiescence +
  `picker_active` + W1 attributed grid; gate on the curated time-series corpus.
- **Phase B — QUESTION sub-classification**: structural fingerprints for rate_limit/auth/permission/
  model_choice/update/error/unknown; opencode Tier-1 handle classifies directly.
- **Phase C — real-time streaming + typing UI**: `appended_text` → live bubble → jsonl swap;
  typing indicator redesign.
- **Phase D — corpus generator**: session-mining + scenario templates + auto-label → scale the
  accuracy set + nightly drift scorecard.
- Tier-1 handles (opencode now) layered in wherever available.

Most primitives exist (P1a SignalExtractor, PaneState enum, agent_state_replay, W1 attributed
grid). This plan is mostly INTEGRATION + the corpus to measure — not a from-scratch build.

<!-- revision: 1 -->
