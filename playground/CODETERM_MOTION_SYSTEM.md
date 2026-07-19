# CodeTerm Motion System

Repository-native motion direction for the CodeTerm product UI. This is the plan
that the [Motion Lab](./README.md) feeds into. The Lab is a sketchbook for one
signal at a time; this document is the system those signals join.

**Scope of authority:** product motion in `src/` and `packages/chat-ui/` (the
React/TS webview and the chat-block package). Tokens live
in [`src/styles/tokens.css`](../src/styles/tokens.css); shared motion CSS lives in
[`src/styles/globals.css`](../src/styles/globals.css) and
[`src/styles/agent-state.css`](../src/styles/agent-state.css).

> How this was inspected: rendered browser capture was unavailable this pass (the
> Claude-in-Chrome extension was not connected), so the audit reads the
> authoritative motion source — every `@keyframes`, `transition`, easing and
> duration in `src/styles/*` and the component tree — cross-checked against a
> read-only live inventory from the Motion Lab (`http://127.0.0.1:1431`) and the
> running dev build. Keyframe source is the ground truth of what actually moves;
> filenames were never the basis for a judgement.

---

## 1. Honest audit of current motion

CodeTerm already has the *ingredients* of a good system and almost none of the
*system*. The token file is thoughtful; adoption is not.

### What is genuinely good (keep and model on it)

- **`src/styles/tokens.css:20-32`** defines a real, restrained scale:
  `50 / 120 / 200 / 320 / 480ms` and five named easings. This is a strong
  foundation — the problem is that most surfaces ignore it.
- **`src/styles/agent-state.css`** is the best motion in the product. The working
  indicator holds three dots across `work → thinking → streaming` and morphs only
  *rhythm and hue* (all eased, `transition: color 0.6s`), never snapping the row;
  it debounces phase ~700ms upstream so it can't strobe; and it degrades to a
  static, legible state under `prefers-reduced-motion`. **This is the reference
  standard for every other indicator in the app.**
- **A global reduced-motion reset exists** (`tokens.css:80-88`). Good baseline,
  though blunt — see §7.

### The core disease: everything "alive" moves differently

The single biggest defect is that "something is happening" is expressed by **~10
unrelated looping keyframes**, each with its own period and curve, all visible at
once in a busy workspace:

| Keyframe | Location | Says |
| --- | --- | --- |
| `ct-dots-squash-jump` (1.05s) | `agent-state.css:110` | agent working |
| `sidebar-workspace-working-pulse` | `globals.css:1804` | workspace active |
| `sidebar-hierarchy-breathe/drift/dot` | `globals.css:2248-2275` | hierarchy alive |
| `pane-focus-pulse` | `globals.css:2458` | pane focused |
| `plan-companion-presence-pulse` | `globals.css:3589` | plan companion present |
| `mem-pulse` / `mem-pulse-card` | `globals.css:2527-2531` | memory active |
| `ct-canvas-pulse/live/heartbeat-dot` | `globals.css:2589-2617` | canvas live |
| `agent-icon-working-spin` / `ct-spin` | `globals.css:2708/2785` | busy |
| `breathing-bar` | `globals.css:2500` | loading |

The Motion Lab's *accepted* "quiet machine" glyphs (a press, a torque driver, a
feed carriage — `playground/src/concepts/*`, 2.8s ambient period) are yet another
dialect that **does not match what ships** (`ct-phase-dots`), and the workspace
card badge is a *third* working signal (three dots pulsing ~1.2s per the live
inventory). Three answers to one question — "is this thing working?" — that never
agree. **A user running many agents sees a room where everything twitches on its
own clock.** That is the opposite of a calm workshop.

### Fragmentation, surface by surface (confirmed inconsistencies)

- **Shared primitives bypassed.** `.ct-btn` uses `transition: all`; `Card.tsx`
  uses unparameterized Tailwind `transition-colors`; many local buttons/menus have
  instant hover paint with no token. `transition`/`transition-colors` appears
  across **50 component files** with no shared curve. There is no single "state
  change" primitive, so no two surfaces change state the same way.
- **Workspace cards** (`src/components/Sidebar/WorkspaceItem.tsx`): 140ms
  background/color/transform, but **selection is an abrupt fill to accent** with no
  causal cue, and the edit control's opacity appears with no transition of its own.
- **Sidebar tabs** (`TabItem.tsx`, `globals.css:1077` `sidebar-tab-appear`): enter
  uses a back/overshoot curve; **close is two timer-coordinated phases** (150ms
  visual exit + 200ms height collapse) — a `setTimeout` choreography that
  violates the repo's no-sleep rule and races.
- **Disclosure** (General / Workspaces / Recently Closed; team headers): the
  centered hairline bends on hover/focus and the label/count crossfades, but **the
  revealed content has no entrance choreography** — it just appears. Chevrons
  rotate on a separate rule. Team-header hover is an instant background swap.
- **Overlays** (`src/components/shared/DropdownMenu.tsx`): portal placement is
  solid, but the surface **appears/disappears with no origin-aware entrance**; only
  item colors transition; submenus replace content abruptly. `ct-menu-pop`
  (`globals.css:920`) exists but is not applied consistently.
- **Toasts** (`globals.css:643-868`, **9 keyframes**): 520ms seed/panel launch,
  delayed 320ms content, 620/720ms dismissal, plus timers. This is a **separate,
  much slower motion dialect** than the rest of the UI and reads as a different
  app. Frequent feedback should never be the slowest thing on screen.
- **Chat / terminal**: mode switch, composer, sent-message follow-scroll,
  expandable command/system blocks, and the new-session portal are **separate
  implementations**. `NewSessionPicker` has focus management but no spatial
  entrance/exit. Pane Chat↔Terminal switch is mostly state replacement (a cut, not
  a move). Terminal resize runs via `requestAnimationFrame`.
- **Chat blocks live in `packages/chat-ui/`** (`@codeterm/chat-ui`), not `src/` —
  `MessageBubble.tsx`, `ToolCallBubble.tsx`, `CodeBlock.tsx`,
  `AttachedDiscussionDoc.tsx`, `ContextRequestCard.tsx`. Expandable bodies are
  conditionally rendered (`{expanded && …}`) so they **snap open/closed with no
  height/opacity transition**; most chevrons rotate by class change with **no
  transition** (only `InlineToolRow` transitions its rotate); tool cards transition
  colors only. The one message enter is a generic `.ct-agent-msg` 300ms opacity
  fade (`agent-state.css:132`) — which disagrees with the 220ms out-expo used by
  the agent-question card. User bubbles are stable with a restrained 150ms
  timestamp hover fade — the one chat surface that's right. **`packages/chat-ui`
  has zero `prefers-reduced-motion` handling** while the rest of the app is
  thorough. One disclosure primitive fixes most of surfaces 7-8 at once.
- **Two chat views disagree on the scroll contract.** `ChatView.tsx` follows with
  `scrollIntoView({behavior:"smooth"})` and a 40px bottom threshold;
  `PaneSessionChat.tsx` snaps instantly (`el.scrollTop = el.scrollHeight`) with a
  2px threshold. Same product gesture, two behaviors. The user's explicit contract
  — **motion must never steal scroll when detached from bottom** — has to hold in
  both.
- **Downstream dialects**: Canvas/Mission Control (`missionControl.css`, ~31 motion
  declarations) and Usage (~27) each carry their own dense language. These are not
  the first normalization target — they are leaf surfaces.

### The quieter, more urgent gap: feedback, not entrances

The loud problem is desynced pulses. The *load-bearing* problem is thinner and
more fundamental — **the app under-responds to the pointer**:

- **101 UI files define hover behavior; only 24 define any active/pressed state;
  only 5 source/style files mention `focus-visible`.** Most controls acknowledge
  the cursor passing over them but not the click landing, and keyboard focus is
  nearly invisible. Apple's *Designing Fluid Interfaces* is explicit that
  immediate touch-down highlighting and continuous feedback are what make an
  interface feel alive under the hand — CodeTerm largely skips the touch-down.
- CodeTerm already has a **`useTouchOptimizedUi` / `touchMode`** path across
  Sidebar, Chat, `TabItem`, tooltips, composer, and disclosures. So we can specify
  *stronger direct-touch press response and quieter mouse/trackpad hover* without
  inventing any new modality detection.
- **Reduced-motion handling is itself fragmented** across `tokens.css`,
  `globals.css`, `agent-state.css`, Canvas, and `ChatSetupState` — so even the
  accessibility floor is inconsistent and must be consolidated in stages (§7).

### Structural facts that shape the plan

- **A production-token `ComponentGallery` route already exists** (`?prototype=
  gallery`) covering the real shared `Button`, `Chip`, `Card`, and `FormRow`
  primitives. The Motion Lab is a separate Vite app. **Decision:** interaction
  states (hover/press/focus/selected) get reviewed on the *real* ComponentGallery
  with production primitives; the Motion Lab stays for *exploratory choreography*
  only. We do not grow the Lab into a second component library (§8).
- **No animation framework is present** — no Framer Motion, Motion One, React
  Spring, or transition-group — and almost no `animationend`/`transitionend`
  ownership code. **Decision:** the substrate is CSS + WAAPI, with lifecycle driven
  by transition/animation events and WAAPI `finished` promises. No framework in the
  early phases; any library must later earn its bundle and complexity cost (§3).

**Verdict:** the fix is not more motion. It is *consolidation plus responsiveness* —
one state-change primitive that finally includes press and focus, one alive-signal,
one overlay grammar, one exit contract — all built on the tokens and the CSS/WAAPI
substrate the repository already has.

---

## 2. Motion principles (grounded in what CodeTerm users actually do)

The user runs many agents across many panes and workspaces, switches context
constantly, and reads streaming output while other agents work. Motion earns its
place only by serving that.

1. **The app breathes as one.** Everything that is merely *alive* — every working
   agent, active workspace, focused pane, present peer — shares a single ambient
   heartbeat: one period, one curve, one amplitude. Liveness is a property of the
   whole room, not a competition between widgets. *(This is the signature move and
   the single largest coherence win.)*

2. **Motion explains causality, or it doesn't ship.** A surface enters *from where
   it was summoned*: a menu grows from its trigger, a disclosure grows from its
   header, a sheet rises from the edge it docks to. If motion can't answer "what
   just happened and why," make the change instant instead.

3. **Continuity over cuts.** Context switches the user makes constantly —
   Chat↔Terminal, tab switch, workspace select — must feel like moving *within one
   space*, not teleporting between screens. Shared elements persist and slide;
   they don't blink out and back in.

4. **Content is sovereign; motion yields to it.** The terminal grid, streaming
   tokens, and scrollback are the product. Motion never animates *over* reading or
   *interpolates* text. Around content, motion is quiet enough to ignore.

5. **Direct manipulation feels physical.** Drag and reorder are pointer-led: a
   lift with weight, neighbors that make room, a settle that absorbs momentum. The
   thing you grabbed stays under your finger.

6. **Quiet by default, expressive only at consequence.** Routine changes are
   near-silent. The small budget for personality (a gentle overshoot, a one-shot
   settle) is spent only where it means something: a task completes, an agent hits
   a blocker, an action fails. Rationing is what makes those moments read.

7. **Latency is the message, so don't hide it.** Where the point of the pixel is
   "the system responded *now*" — keystroke echo, cursor, terminal output, hover —
   motion is instant. Interpolating those would fake responsiveness the product
   sells for real.

---

## 3. Token set (adopt and extend the existing scale — do not invent a new one)

CodeTerm already ships the right numbers. The work is **semantic assignment and
adoption**, plus filling three real gaps (an exit curve, distance, and a single
ambient period). Keep the five durations and four of the five easings; cut the
redundant one.

### Durations — keep as-is, assign meaning (`tokens.css:20-25`)

| Token | Value | Semantic use |
| --- | --- | --- |
| `--ct-duration-instant` | `50ms` | Hover paint, press feedback, color-only change. Feels immediate. |
| `--ct-duration-fast` | `120ms` | Small in-place state: selection, chevron rotate, badge swap, checkbox. |
| `--ct-duration-normal` | `200ms` | Element enter/exit: disclosure, menu, popover, tooltip, tab appear. |
| `--ct-duration-smooth` | `320ms` | Larger spatial moves: modal/sheet, mode switch, reorder settle. |
| `--ct-duration-dramatic` | `480ms` | **Rare.** Full-surface or first-run transitions, empty→populated. |

### Easings — keep four, cut one, add the missing exit curve

| Token | Curve | Semantic use |
| --- | --- | --- |
| `--ct-ease-out-expo` | `cubic-bezier(0.16,1,0.3,1)` | **Default entrance.** Arrives fast, settles soft. |
| `--ct-ease-decel` | `cubic-bezier(0,0,0.2,1)` | Gentle enter where expo is too aggressive (dense rows). |
| `--ct-ease-in-out-quint` | `cubic-bezier(0.86,0,0.07,1)` | Symmetric on-screen *moves*: reorder, mode switch, follow-scroll. |
| `--ct-ease-out-back` | `cubic-bezier(0.34,1.56,0.64,1)` | **Consequence only.** One-shot overshoot: completion, toast land. |
| ~~`--ct-ease-spring`~~ | `(0.175,0.885,0.32,1.1)` | **Cut.** Redundant with `out-back`; consolidate call-sites onto it. |
| `--ct-ease-accel` **(new)** | `cubic-bezier(0.4,0,1,1)` | **Exits.** Accelerate off-screen. Currently missing — everything exits on the wrong curve. |

### Distance — new, small, semantic (currently every surface picks its own px)

```css
--ct-shift-xs: 2px;   /* press nudge, micro-settle */
--ct-shift-sm: 4px;   /* list-item / disclosure content enter */
--ct-shift-md: 8px;   /* menu / popover offset from trigger */
--ct-shift-lg: 16px;  /* sheet / modal rise */
```

### Opacity & ambient — one heartbeat for the whole app (new)

```css
--ct-fade-muted: 0.55;      /* matches existing text-secondary opacity */
--ct-motion-ambient: 1400ms;      /* THE shared liveness period — one source */
--ct-motion-ambient-ease: cubic-bezier(0.37,0,0.63,1);  /* symmetric breathe */
--ct-motion-scale: 1;             /* dev-only global tempo multiplier — see §8 */
```

Every ambient loop derives its `animation-duration` from
`calc(var(--ct-motion-ambient) * var(--ct-motion-scale))`. Nothing hardcodes a
looping period again.

**One rule that fixes ~90% of the fragmentation:** raw millisecond values, bare
`ease`/`ease-out` keywords, hand-retyped cubic-beziers, and Tailwind's default
`transition-colors` (150ms) are the disease — the audit found ~24 distinct
durations and 5 stray non-token curves against this 5-value scale. Every new or
touched transition references a token; a raw duration or a bare keyword is a
review defect, not a style choice.

### Substrate: CSS + WAAPI, no framework

The repo ships **no** animation library (Framer Motion, Motion One, React Spring,
transition-group are all absent) and almost no `animationend`/`transitionend`
ownership. Keep it that way for the early phases:

- **Steady-state and simple transitions → CSS**, driven by the tokens above.
- **Lifecycle that must wait for motion to finish** (exit-then-unmount, sequenced
  reveals) → **WAAPI `element.animate(...).finished`** or a `transitionend`/
  `animationend` listener. This is also what the repo's **no-sleep rule** demands:
  the current `setTimeout`-coordinated exits (tab close `App.tsx:93`, toast
  `NotificationToast.tsx:16`) treat elapsed time as readiness and must move to
  event/promise completion. **No duration may be duplicated across JS and CSS to
  "stay in sync"** — the CSS owns timing; JS waits on the event.
- A framework may be revisited only if orchestration outgrows this substrate, and
  only if it earns its bundle and complexity cost. It does not in phases 0-5.

---

## 4. Element / state behavior matrix

Curves and durations reference §3 tokens. "Instant" means no transition.

| Surface / element | Trigger | Behavior | Duration · Easing |
| --- | --- | --- | --- |
| Any control | hover / unhover | color/background paint only; quieter under `touchMode` | instant · — |
| Any control | press / touch-down | depress `--ct-shift-xs` (scale/translate); **stronger under `touchMode`** — immediate touch-down feedback | instant · — |
| Any control | keyboard focus | single shared `:focus-visible` ring, paint-in | instant · — |
| Workspace card | select | selected bg cross-fades **and** a left accent rail scales in (`scaleY`) from the pointer's row — the rail *is* the causal cue, not an abrupt fill | fast · out-expo |
| Workspace card | activity/working | shared ambient heartbeat (§5), not a bespoke pulse | ambient · ambient-ease |
| Workspace card | edit control reveal | opacity + `--ct-shift-xs` slide, tokened | fast · decel |
| Disclosure (General / Workspaces / Recently Closed) | expand/collapse | `grid-template-rows 0fr→1fr` reveal + content fades/rises `--ct-shift-sm`; chevron rotates in lockstep on the **same** curve | normal · out-expo (in), accel (out) |
| Team / group header | hover | background paint | instant · — |
| Team disclosure | expand | same disclosure primitive as above; hairline is a static divider, not a hover toy | normal · out-expo |
| Sidebar tab | appear | rise `--ct-shift-sm` + fade | normal · out-expo |
| Sidebar tab | close | fade + collapse via **one** clip/`grid-rows` transition, ended on `transitionend` (no `setTimeout` pair) | normal · accel |
| Sidebar tab / workspace | reorder (drag) | lift (scale `1.02` + shadow), neighbors part via transform, drop settles (FLIP) | smooth · in-out-quint |
| Command row | hover / active | paint; active row gets the accent rail (same primitive as card select) | instant / fast |
| Dropdown / context menu | open | grow from trigger: `transform-origin` at anchor, scale `0.96→1` + fade + `--ct-shift-md` | normal · out-expo |
| Dropdown / context menu | close | reverse, faster | fast · accel |
| Submenu | open | slide `--ct-shift-sm` from parent edge + fade, not a content swap | normal · out-expo |
| Tooltip | show (after delay) | fade + `--ct-shift-xs` from anchored edge | fast · decel |
| Toast | enter | rise `--ct-shift-lg` + fade, one gentle `out-back` settle | normal · out-back |
| Toast | dismiss | fade + slide out | fast · accel |
| New-session modal | open | backdrop fades; panel rises `--ct-shift-lg` from center-bottom | smooth · out-expo |
| Modal dropdowns | open | menu grammar above (origin-aware) | normal · out-expo |
| Top bar / Chat↔Terminal switch | switch | cross-slide the pane body one direction (content continuity), shared chrome stays put | smooth · in-out-quint |
| Chat: send message | submit | composer content commits; sent bubble lands at thread end with a small settle | fast · out-back (settle) |
| Chat: follow-scroll | new content while pinned to bottom | thread eases up (one unified contract for both `ChatView` and `PaneSessionChat`); **never** steals scroll when detached from bottom | smooth · in-out-quint |
| Chat: user / assistant / system / command / tool block | append | block appears at final position, fade only — **no per-token animation, no layout bounce** | fast · decel |
| Chat: expandable long content | show / hide | disclosure primitive (`grid-rows`) | normal · out-expo / accel |
| Terminal | output / cursor / keystroke | **instant** — never interpolated | — |
| Terminal | resize | driven by layout (rAF acceptable); no decorative motion | — |
| Terminal | reconnect / status change | status glyph joins the ambient heartbeat; a one-shot settle on reconnect success | ambient / fast · out-back |
| Mobile sidebar | open / close | slide from edge + backdrop fade | smooth · out-expo (in), accel (out) |
| Any "alive" indicator | while active | **shared ambient heartbeat only** (§5) | ambient · ambient-ease |
| Loading / skeleton | pending | single breathing-bar shimmer on the shared ambient period | ambient · ambient-ease |
| Empty state | mount | one-shot fade + `--ct-shift-sm` rise | normal · out-expo |
| Error state | appear | one-shot settle (`out-back`), color to danger; no looping shake | normal · out-back |

---

## 5. Spatial & causal rules

- **Overlays are origin-true.** Menus, popovers, tooltips, and submenus set
  `transform-origin` to the anchoring edge/corner and grow from it. A menu that
  fades in centered is a bug — it hides where it came from. Consolidate on one
  primitive; retire the ad-hoc appear/disappear in `DropdownMenu.tsx` and the
  orphaned `ct-menu-pop`.
- **Disclosure grows from its header.** Reveal via `grid-template-rows: 0fr→1fr`
  (animatable, only the disclosed region reflows — acceptable for an explicit user
  action, never a hot path). Content fades and rises `--ct-shift-sm` underneath a
  clip. Chevron rotation shares the exact curve and duration so header and body
  read as one gesture.
- **Selection is a rail, not a flood.** The active workspace/row is marked by an
  accent rail that scales in from the interaction point, plus a background
  cross-fade. This shows *which* item became active and that *you* did it — an
  instant fill shows neither.
- **Reorder obeys the finger.** FLIP: the dragged item lifts (subtle scale +
  shadow) and stays under the pointer; displaced neighbors translate to make room;
  the drop absorbs momentum with `in-out-quint`. No teleport-to-slot.
- **Sending launches, then the thread settles.** On submit, the composer commits
  and the new bubble lands at the thread end with a small `out-back` settle; if the
  view is pinned to bottom, follow-scroll eases up on `in-out-quint`. This replaces
  the toast-grade choreography with something proportionate.
- **Streaming is stillness plus a heartbeat.** While an agent streams, the only
  motion is the shared ambient heartbeat and (if pinned) eased follow-scroll. Text
  never fades in per token; blocks never bounce on append.
- **Status joins the heartbeat.** Working / active / focused / present / live all
  render as the *same* breathe on the *same* period. State *transitions* (idle→
  working, connecting→connected) get one eased settle; steady states just breathe.
- **Mode & tab switches move within one room.** Chat↔Terminal and tab changes
  cross-slide the body while shared chrome holds still — continuity, not a cut.

---

## 6. Where motion is forbidden or must be instant

- **Terminal grid, cursor advance, keystroke echo, scrollback** — never
  interpolated. This is the product's core latency signal.
- **Streaming token append** — instant. No per-glyph or per-token fade.
- **Search / filter results** — appear instantly; no staggered list reveal while
  the user is narrowing.
- **Numeric/status value updates** (token counts, timers, sizes) — instant. No
  odometer rolls.
- **Hover paint on dense lists** (tabs, command rows, mem/log rows) — `instant`
  (≤50ms). Staggered or springy hovers on dense rows read as noise.
- **Hot-path commands** (fire on tab switch / mount / render) — motion must be
  cheap and bounded; never block the async runtime (see AGENTS.md async
  discipline). If a transition would gate a hot path, drop it.
- **No looping attention-grabbers** — no infinite shake/bounce/spin as "error" or
  "look here." One-shot settle, then rest.
- **Never animate layout properties** (`left`, `top`, `width`, `height`,
  `padding`) — they relayout every frame. Migrate the known offenders to
  `transform`/`opacity`: `Segmented.tsx` thumb animates `left` (→ `translateX`);
  the voice orb animates `width/height` (→ `scale`); tab-close phase 2 animates
  `height/padding` (→ clip or `grid-rows`). The disclosure `grid-rows 0fr→1fr` is
  the one sanctioned exception, gated to explicit user actions off the hot path.

---

## 7. Reduced-motion

The global reset (`tokens.css:80-88`) currently flattens *all* animation and
transition to `0.01ms`. That is safe but over-broad — it also kills opacity
crossfades, which are not vestibular triggers and carry useful state meaning.

**Refine to:** under `prefers-reduced-motion: reduce`,

- disable all **transforms, translation, scale, and infinite ambient loops**;
- **keep short opacity transitions** (≤`--ct-duration-fast`) so state changes
  still read;
- guarantee every ambient indicator has a **static, legible resting state** —
  `agent-state.css:208-222` already does this exactly right and is the pattern to
  copy for the consolidated heartbeat and every status glyph.

The reduced-motion state is not a degraded fallback; it is a first-class rendering
that must still answer "what is working / selected / open" without any movement.

**Consolidate the fragmented coverage, in stages.** Reduced-motion is currently
enforced in ~13 CSS blocks plus JS paths (`useReducedMotion.ts`, `ChatSetupState`,
Canvas), which is why behavior differs by surface — and **`packages/chat-ui` has
none at all**, so every chat disclosure and message fade ignores the preference.
The refined global block (transforms/loops off, short opacity kept, static resting
state guaranteed) becomes the single baseline; per-surface blocks are removed as
each surface adopts a primitive; and `packages/chat-ui` gets first-class handling
when its disclosure primitive lands (Stage 6). Where JS gates motion, it reads the
one shared `useReducedMotion` hook — not its own media query.

---

## 8. Art-direction loop (manual — no Playwright motion conformance)

Automated conformance was deliberately removed and is **not** coming back. Motion
quality is judged by eye, in context, against a plain-language gate.

1. **Sketch in the Lab, judge in the product.** Use the Motion Lab
   (`http://127.0.0.1:1431`) to compare variants of *one* signal side by side.
   But acceptance happens on the **real surface in the running dev build**
   (`http://127.0.0.1:7686`), never on the Lab card alone — the Lab scales glyphs
   3× and lies about final size.
   - **Two workbenches, two jobs.** Interaction states (hover / press / focus /
     selected / disabled) are reviewed on the existing **production
     `ComponentGallery` route** (`?prototype=gallery`, real `Button` / `Chip` /
     `Card` / `FormRow`), extended with those states — because those must be
     judged on the *actual* primitives. The **Motion Lab stays for exploratory
     choreography only** (ambient signals, novel transitions). We do **not** grow
     the Lab into a second component library; that duplicates the gallery and lets
     the two drift.
2. **Real-time then slow-motion, in situ.** Add a dev-only `--ct-motion-scale`
   root variable (default `1`) that every token multiplies through. A dev toggle
   sets it to `4` (0.25× speed) so any *real* transition — a menu opening, a tab
   closing, the heartbeat — can be watched slowly on the actual component, then
   returned to `1`. This is the Lab's tempo slider promoted to the product; it
   needs no test harness.
3. **The gate — three questions, yes to all three:**
   - *Causality:* can I tell what just happened and that I caused it?
   - *Quiet:* can I ignore it while reading the terminal next to it?
   - *Better than instant:* is this genuinely clearer than a cut?
   If any answer is no, make it instant or revise. "Technically correct" never
   promotes a weak motion.
4. **Before/after, side by side.** Capture the old and new behavior as short
   screen recordings of the same interaction, same theme, and review them
   together. The art director accepts only when the improvement is visible without
   narration. No checklist, no numeric threshold, no bureaucracy.
5. **One reviewer owns the call.** Subjective taste is the point. The gate keeps it
   honest; it does not outsource the judgement.

---

## 9. Staged implementation plan (by user value & risk)

Ordered so the highest-visibility, lowest-risk coherence wins land first. Each
stage is small, reusable, and names concrete source surfaces. **This document is
the plan only — production motion is not to be broadly implemented yet.**

### Stage 0 — Foundation (low risk, enables everything)
- Finalize tokens in `src/styles/tokens.css`: add `--ct-ease-accel`, the
  `--ct-shift-*` scale, `--ct-motion-ambient(+ease)`, `--ct-motion-scale`; remove
  `--ct-ease-spring` and migrate its call-sites to `--ct-ease-out-back`.
- Refine the reduced-motion global block per §7; wire the dev `--ct-motion-scale`
  slow-motion toggle (§8).
- Commit to the CSS/WAAPI substrate (§3) — no framework.
- Build the small primitive set (below). No surface behavior changes yet.

**Reusable primitives (the whole system rides on these):**
1. `usePressFocus` / primitive states — token press-depress, `:active`, and a
   single visible `:focus-visible` ring. Consumers: `Button`, `Chip`, `Card`,
   `FormRow`, every clickable row.
2. `<Disclosure>` / `useDisclosure` — tokened `grid-rows` reveal + chevron in
   lockstep. Consumers: sidebar sections, team groups, chat expandable blocks.
3. `<Popover>` — origin-aware pop. Consumers: `DropdownMenu`, context menus,
   submenus, model pickers, tooltips.
4. `<Overlay>` — backdrop fade + panel rise, exit via WAAPI `finished`. Consumers:
   `NewSessionPicker`, modals, mobile sidebar, sheets.
5. `useReorder` — FLIP lift/part/settle. Consumers: sidebar tabs, workspace cards.
6. `AmbientHeartbeat` — one keyframe + one context driving every "alive" state.

### Stage 1 — Interaction parity: press & focus (highest value, lowest risk)
The audit's sharpest gap: **101 files handle hover, 24 handle press, 5 handle
`focus-visible`.** Fix this first — it's pure shared-primitive work, needs no
entrance choreography, and is both an accessibility fix and the Apple *Fluid
Interfaces* "immediate touch-down feedback" that makes an interface feel alive.
- Add token press-depress + a single `:focus-visible` ring to `Button`, `Chip`,
  `Card`, `FormRow`, and clickable rows; normalize the scattered `active:scale-*`
  values (0.90/0.95/0.97/0.98) onto one.
- Use the existing **`useTouchOptimizedUi` / `touchMode`** path to make direct
  touch press stronger and mouse/trackpad hover quieter — no new modality
  detection. Review on the production `ComponentGallery` (§8).

### Stage 2 — Unify the heartbeat (signature coherence win)
Collapse the ~10 pulse keyframes (§1 table) onto `AmbientHeartbeat`. Concrete
targets: `agent-state.css` phase dots, `sidebar-workspace-working-pulse`,
`sidebar-hierarchy-*`, `pane-focus-pulse`, `plan-companion-presence-pulse`,
mesh presence, `ct-canvas-*`. Visible on nearly every screen; the single biggest
"this is one app" win.

### Stage 3 — Selection & disclosure causality (high daily value)
Accent-rail selection for `WorkspaceItem.tsx` and command rows; `<Disclosure>` for
General / Workspaces / Recently Closed and team groups (their bodies are currently
instant DOM swaps). Retire per-surface selection fills and the hover-only hairline.

### Stage 4 — Overlay grammar + kill the timers (medium risk)
Route `DropdownMenu.tsx`, submenus, `NewSessionPicker`, and tooltips through
`<Popover>`/`<Overlay>`. Replace every `setTimeout`-coordinated exit and every
JS/CSS duplicated duration (tab close, toast `DISMISS_ANIMATION_MS`, mobile
sidebar 300ms) with `transitionend` / WAAPI `finished` — satisfies the no-sleep
rule and removes the races.

### Stage 5 — Tab lifecycle
`sidebar-tab-appear` + the two-phase (150ms + 200ms) close onto `useReorder` +
`<Disclosure>` exit. One transition, ended on an event, no timer pair, no animated
`height/padding`.

### Stage 6 — Chat (`packages/chat-ui`): the biggest daily surface
- `<Disclosure>` across `ToolCallBubble`, `CodeBlock`, `AttachedDiscussionDoc`,
  `ContextRequestCard`, system-prompt cards — every expandable body currently
  snaps.
- Send-settle + `in-out-quint` follow-scroll; **unify the two scroll contracts**
  (`ChatView` smooth/40px vs `PaneSessionChat` snap/2px) onto one, and hold the
  "never steal scroll when detached" rule in both.
- Give `packages/chat-ui` its **first `prefers-reduced-motion` handling** (via the
  shared hook).
- Keep assistant streaming text and terminal output **unanimated**.
- **Bring toasts down to system tempo** — replace the 460/520/620/720ms dialect
  (`globals.css:643-888`, `NotificationToast.tsx`) with `normal · out-back` in /
  `fast · accel` out.
- Migrate the layout-property offenders (`Segmented` `left`, voice orb `w/h`).

### Stage 7 — Downstream dialects (deferred)
Canvas/Mission Control (`missionControl.css`) and Usage normalize last, against the
now-stable system. They are leaf surfaces, not the seam.

---

## 10. Before / after acceptance examples

Concrete pairs that make subjective quality reviewable. Accept when the "after"
passes the §8 gate on the real surface.

**A. Workspace selection**
- *Before:* click a card → background snaps to accent fill. No sense of which
  changed or that you did it; in a long list the eye has to re-find the selection.
- *After:* selected background cross-fades (`fast · out-expo`) while an accent rail
  scales in from the clicked row. *Accept when* a bystander watching over your
  shoulder can name which card you clicked without being told.

**B. "Is anything working?"**
- *Before:* three agents, a plan companion, and a live canvas each pulse on their
  own period and curve — the workspace flickers like a slot machine.
- *After:* all of them breathe on the one ambient period; the room feels calm and
  clearly *alive*. *Accept when* you can glance away, glance back, and read
  "several things are working" as one steady state, not a swarm.

**C. Context menu**
- *Before:* right-click → menu fades in centered on its box; you lose track of what
  you clicked.
- *After:* menu grows from the click point (`transform-origin` at anchor,
  `0.96→1` + `--ct-shift-md`, `normal · out-expo`). *Accept when* the menu visibly
  belongs to the thing you invoked it on.

**D. Closing a tab**
- *Before:* two `setTimeout` phases (150ms fade + 200ms collapse) that can race and
  occasionally leave a gap or double-collapse.
- *After:* one clip/`grid-rows` collapse on `--ct-ease-accel`, completion driven by
  `transitionend`. *Accept when* it never leaves a visual gap and reads as a single
  motion — and when there is no timer in the code path.

**E. Toast vs. the rest of the UI**
- *Before:* a toast takes 520ms to launch and 620–720ms to dismiss while the menus
  next to it move in 200ms — it reads as a different, slower app.
- *After:* toast enters `normal · out-back`, exits `fast · accel`. *Accept when* a
  toast no longer feels heavier than opening a menu.

**F. Press & focus feedback (Stage 1)**
- *Before:* click a sidebar row or `Button` and the cursor-down isn't
  acknowledged; tab through the UI and you can't tell what's focused (only 5 files
  handle `focus-visible`).
- *After:* every control depresses on touch-down (stronger under `touchMode`) and
  shows one consistent focus ring on keyboard nav. *Accept when* the UI feels like
  it answers the finger the instant you press, and you can operate it entirely by
  keyboard without losing your place.

---

## 11. Signature — what makes this motion *CodeTerm's*

Three things, and nothing decorative beyond them:

1. **The app breathes as one.** A single ambient heartbeat across every live
   signal. In a product whose whole job is *many agents working at once*, a unified
   pulse is the identity — the room is busy, and calm about it.
2. **Origin-true motion.** Everything grows from where it was summoned. The
   interface never teleports; it always shows its causal chain.
3. **Terminal-native restraint.** Motion respects the monospace grid and the
   stream. It never interpolates text, never animates over reading, and treats
   instant response as a feature, not a gap to paper over.

Quiet, causal, continuous, and built entirely on tokens the repository already
owns.

<!-- revision: 1 -->
