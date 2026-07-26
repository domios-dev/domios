# Motion system → production integration handoff

Prepared by the Motion Design Lead for the frontend integration lane. **No production
code was edited to produce this document.** Everything below is either an existing
playground artifact or a verified observation about `src/`.

Playground gallery (the visual contract): `http://127.0.0.1:1431` — the component
catalog is the only view. Start it with
`npx vite --config playground/vite.config.ts`.

---

## 0. State of play — production migration has already started

**Important for whoever picks this up:** part of this system is already in `src/`,
landed by another lane. Do not re-implement it; extend it.

| Already in production | Evidence |
| --- | --- |
| `ActivityDots` primitive (the dots component) | `src/components/shared/ActivityDots.tsx` |
| Var-driven dot geometry + tones + rhythms | `src/styles/agent-state.css:59-165` (`--activity-dot-size/-gap/-jump/-duration/-delay`, `.activity-dots--{micro,small,medium}`, `--{accent,thinking,muted}`, `--{work,thinking,streaming}`, `--idle`) |
| Consumers wired | `src/components/shared/BusyState.tsx:44`, `src/components/Sidebar/TabItem.tsx:93`, `src/components/Sidebar/TeamUnit.tsx` |
| Per-context size overrides | `src/styles/globals.css:1866-1868` (2.5px/1.25px jump), `:1966` (3px) |
| Liquid motion tokens | `src/styles/tokens.css:26-27` (`--ct-duration-liquid-hover/-press`), `:35-36` (`--ct-ease-hover-swell`, `--ct-ease-liquid-settle`) |
| Tests guarding it | `src/components/shared/ActivityDots.test.tsx`, `BusyState.test.tsx`, `TeamUnit.activity.test.tsx` |

**Known intentional divergence:** production `micro` keeps all three dots
(`ActivityDots.test.tsx:44` asserts it). The playground `micro` degrades to a single
breathing dot. See §2 for which one I recommend and why.

---

## 1. The oversized three-dot indicator — diagnosis

Two separate defects. The second is the one that looks "oversized" in chat.

### 1a. Verified: `BusyState` hardcodes `size="medium"` for every busy surface

`src/components/shared/BusyState.tsx:44-48` always passes `size="medium"`
(5px dot + 3px jump = 8px tall) regardless of context, including `inline` usage
next to ~13px body text.

I rendered the real `src/styles/agent-state.css` headless at 3× with both sizes
beside identical text. `medium` reads as a heavy triple-bullet that competes with
the label; `small` (3px + 2px jump) sits correctly on the baseline. Repro harness:
`<scratchpad>/dots-prod.html` (loads production CSS directly, no build).

**Fix:** make size context-driven, not constant. Recommended mapping —
- `inline` / chat status rows / composer → `small`
- standalone block busy rows (empty states, panels) → `medium`
- badges inside 40px-class chrome (workspace cubes, tab rows) → `micro`

Implement as `BusyStateProps.dotSize?: ActivityDotsSize` defaulting to
`inline ? "small" : "medium"`, so existing call sites keep working and nothing
silently changes for block usage.

### 1b. Latent: dual-class cascade collision (this is the giant-dots failure mode)

`ActivityDots` emits **both** `activity-dots` and `ct-phase-dots`
(`ActivityDots.tsx:26-27`). `.ct-phase-dots` itself declares
`--activity-dot-size: 4px` (`agent-state.css:60`) while `.activity-dots--medium`
declares `5px` (`:133`). Identical specificity (0,1,0) → **source order alone**
decides which wins. Any future rule ordering change, file split, or an added
`.ct-phase-dots` block lower in the cascade silently resizes every dot in the app.

This is exactly the class of bug that produced the giant dots in the playground
plan card: `.ds-plan-entry__donut i` overrode descendant `i` dimensions
(fixed in `eb96f0048`). A bare-`i` or duplicate-var-owner selector is the hazard.

**Fix:** one owner for the geometry vars. Either drop `--activity-dot-size` from
the `.ct-phase-dots` rule and let only `.activity-dots--{size}` set it (keeping
`ct-phase-dots` for legacy layout/colour only), or drop the compat class from the
component once no CSS depends on it. Add a guard test asserting
`.ct-phase-dots` declares no `--activity-dot-*`.

**Hardening rule for the whole migration:** never style bare `i`/`em` from a
container; never let two selectors of equal specificity own the same geometry var.
Component internals must be addressed via their own class
(`.activity-dots i`), and consumers may only set the documented `--activity-dot-*`
vars.

---

## 2. Artifacts I created (source of truth for intent)

### Primitives / tokens
| Artifact | Path | Notes |
| --- | --- | --- |
| DS token sheet | `playground/src/ds/tokens.css` | GitHub-dark defaults derived from `src/lib/themeCss.ts`; spacing/radii/control-height/typography adopted from the external zip after cross-check; liquid curves + durations. Runtime `--ct-*` themes still override — never ship a static `themes.css`. |
| Living dots | `playground/src/ds/LivingDots.tsx`, `living-dots.css` | Props: `size` micro/small/medium, `tone` accent/thinking/muted, `rhythm` work/thinking/streaming, `idle`. Geometry from `--living-dot-*` vars; `flex:none` so a flex parent can't stretch it. |
| Segmented donut | `playground/src/ds/SegmentedDonut.tsx`, `segmented-donut.css` | Standalone multi-status conic ring. |
| Plan status breakdown | `playground/src/ds/PlanStatusBreakdown.tsx` | "N todo · N review · N blocked · N done" with per-status colours. |
| Round chat button | `playground/src/ds/ChatRoundButton.tsx`, `chat-round-button.css` | Round-vs-square rule documented in `playground/CHAT_BUTTON_CONCEPT.md`. |
| Voice recording family | `playground/src/ds/VoiceRecording.tsx`, `voice-recording.css` | Full capture flow, props-driven. |
| Composites | `AgentStatusRow`, `AgentTreeRow`, `PlanEntryCard`, `WorkspaceCubeGrid` (+ CSS) | Reference compositions, not necessarily the shipping shape. |

### Motion language (named tokens — use these, never inline beziers)
- `--ct-ease-squash-jump: cubic-bezier(.22,.9,.25,1)` — the dots' personality, lifted from the shipped `ct-dots-squash-jump`.
- `--ct-ease-hover-swell: cubic-bezier(.3,1.6,.4,1)` — single-overshoot swell (already in prod tokens).
- `--ct-ease-liquid-settle`, `--ct-ease-liquid-breathe`.
- `--ct-duration-liquid-hover: 380ms`, `-press: 280ms` (already in prod), `-sweep: 560ms`, `-donut: 420ms`, `--ct-duration-recording-pulse: 1200ms`.

**Gesture grammar:**
- Hover = centre-origin scale ~1.02 via *transition* (never a keyframe — keyframes restart on pointer micro-moves and read as a jerk). Dense sidebar surfaces use half amplitude (~1.01).
- Press = centre squash `scale(.955)` at 70ms; release = one pop through rest size.
- Travel (thumbs, segmented, toggles) = **one** keyframe gesture coupling translate+scale: stretch in flight → splat against the destination wall → relax. Track must `overflow:hidden` so nothing escapes.
- Consequence beats (one-shot, never looping): completion light sweep, counter squash-pop, lock-pop.
- Only two things loop: the living dots and the recording pulse.

### Documentation artifacts
- `playground/VOICE_INTERACTION.md` — the voice state machine preservation doc: states (idle/starting/hold/locked/preview/busy), gesture thresholds (**48px up = lock, 36px left = cancel**), warmup release/lock race, pointer-cancel behaviour, retained-clip preview semantics, with `file:line` cites. **Read before touching voice.**
- `playground/CHAT_BUTTON_CONCEPT.md` — round vs rectangular rule as shipped.
- `playground/DS_ZIP_AUDIT.md` — external design-system audit: what to adopt vs discard. Their static `themes.css` must not be adopted; logo is byte-identical to ours; extension icons are *not* byte-identical to anything in repo/plugins (provenance unverified, treat as derived).

### Verification tooling
- `playground/src/gallery/useAnimationFreeze.ts` + `AnimationInspector.tsx` — pauses and seeks real `Animation` objects (`getAnimations({subtree:true})`), so any component's CSS animation can be scrubbed and printed as a 6-frame filmstrip with correct staggers. Works on any specimen; also usable as the review tool for production components if pointed at them.
- Headless frame-freeze harness pattern: `animation-play-state:paused` + negative `animation-delay` per instance → one screenshot = a film strip. See `<scratchpad>/frames.html`.
- Headless screenshot review: `chrome --headless=new --virtual-time-budget=N --screenshot=... <url>` (the browser extension is not connected in this environment).

---

## 3. Intended states per surface

| Surface | States that must exist | Motion |
| --- | --- | --- |
| Agent status row | work / thinking / streaming / idle | Dots only; rhythm + hue morph between phases (eased, phase debounced ~700ms upstream so it cannot strobe); idle settles to static muted dots and **keeps the slot** so the message never jumps. |
| Agent tree row | idle / working × selected / unselected | Agent icon stays static in its slot (no spin); muted dots appear **inline after the title**. |
| Plan entry card | mixed / blocked present / all-done / empty (0/0) / working / selected | Donut segments ease in on `--ct-duration-liquid-donut`; all-done fires one light sweep + counter pop; working shows small muted dots after the title. |
| Workspace cube | selected / unselected / working / hover / press | 18×9 badge at (3,3) with micro dots — must not cover the centred initials; press = centre squash. |
| Round chat buttons | rest / hover / active / disabled / recording | Centre squash press; recording pulse breathes on the shared curve. |
| Voice | idle / recording / sliding-to-cancel / locked / preview / cancel-armed | Per `VOICE_INTERACTION.md`; keep lock-pop. |
| Segmented / toggles | per-option, on/off | One liquid travel gesture, wall-bounce, clipped by track. |

**Do not animate:** terminal grid, cursor, keystroke echo, streaming token append,
numeric/status counters, search/filter results. Instant is a feature there.

---

## 4. Accessibility & reduced motion

- `prefers-reduced-motion: reduce` must leave a **legible static state**, not an
  empty slot. Production already does this correctly at
  `agent-state.css:251-259` (dots `animation:none`, `opacity:.85`, no transform) —
  that is the pattern to copy for every new component.
- Dots are decorative: `aria-hidden="true"` on the dots span; the *status text*
  carries meaning via `role="status"` + `aria-live="polite"` on the row
  (`BusyState.tsx:38-41` is correct). Never make the dots the only signal.
- Keep `aria-label` on the row equal to the human-readable status.
- Every interactive port needs a visible `:focus-visible` ring and real
  roles/names — Andrey tests with Playwright, so semantics must not regress.
  (Playwright is suspended per `AGENTS.md`; do not add/run motion-conformance
  specs. Behaviour changes get focused unit tests instead.)
- Reduced-motion must also gate JS-driven motion through the existing shared
  `useReducedMotion` hook, not a per-component media query.

---

## 5. Migration order (each step independently shippable)

1. **Dots correctness** (highest value, smallest diff)
   - Fix 1a: context-driven `dotSize` in `BusyState`.
   - Fix 1b: single owner for `--activity-dot-*`; add the guard test.
   - Sweep remaining bespoke working indicators onto `ActivityDots`: the
     ~10 unrelated pulse keyframes in `globals.css` (`sidebar-workspace-working-pulse`,
     `sidebar-hierarchy-*`, `pane-focus-pulse`, `plan-companion-presence-pulse`,
     `mem-pulse*`, `ct-canvas-pulse/live/heartbeat-dot`, `agent-icon-working-spin`,
     `breathing-bar`). Do this in small batches, one surface per commit.
2. **Token consolidation** — finish adopting the remaining liquid tokens into
   `src/styles/tokens.css` (sweep/donut/recording-pulse durations,
   `--ct-ease-squash-jump`, `--ct-ease-liquid-breathe`) and replace inline beziers
   at call sites. No visual change intended in this step.
3. **Press/focus parity** — the shared-primitive gap: 101 files handle hover,
   ~24 handle press, ~5 mention `focus-visible`. Add centre-squash press +
   one `:focus-visible` ring to `.ct-btn`, `Card`, `Chip`, `FormRow`, clickable
   rows; normalise the scattered `active:scale-*` values onto one. Use the
   existing `useTouchOptimizedUi`/`touchMode` path for stronger touch press and
   quieter pointer hover — no new modality detection.
4. **Round chat buttons** — align composer buttons with `CHAT_BUTTON_CONCEPT.md`.
5. **Plan entry card** — segmented donut + status breakdown + all-done sweep.
6. **Voice** — only after re-reading `VOICE_INTERACTION.md`; preserve thresholds
   and the warmup race behaviour exactly. Highest regression risk in the whole
   migration; consider it last and gate it on manual gesture testing.
7. **Downstream dialects** — Canvas/Mission Control and Usage normalise last.

**Sequencing rule:** never mix a token refactor and a behaviour change in one
commit. Token steps must be visually inert; behaviour steps must be small enough
to eyeball.

---

## 6. Focused verification per step

Commands (Windows: drive these through `.\build.ps1`):
- `npx vite build --config playground/vite.config.ts` — playground still builds.
- `./build.sh check` — Rust/TS typecheck for the app.
- Focused unit tests only, one at a time: `ActivityDots.test.tsx`,
  `BusyState.test.tsx`, `TeamUnit.activity.test.tsx` plus any test you add.
  Run Rust tests only via `scripts/test-capped.sh`.
- Dev instance: `./build.sh dev` (port **7686** — never touch 7685 prod), then
  confirm the running build matches source:
  `curl -s http://127.0.0.1:7686/api/health | jq '.build'` vs `git rev-parse HEAD`.

Manual visual gate (motion is judged by eye, not by conformance test):
1. Chat status row at real size — dots must sit on the text baseline and not
   out-weigh the label; compare against `<scratchpad>/dots-prod.html`.
2. Sidebar with several agents working — one calm room, no strobe: same period,
   offset phase per instance, tiered amplitude.
3. Tab through the changed surfaces — focus ring visible everywhere.
4. OS reduced-motion on — every indicator still legible and static.
5. Light theme — dots and badges must stay contrasty (tone derives from
   `currentColor`/`--ct-fg`, so verify no hardcoded hex crept in).
6. Scrub each ported component in the gallery inspector and check the filmstrip
   has no dead or duplicated frames.

**Three questions gate every motion change:** can I tell what happened and that I
caused it? can I ignore it while reading the terminal next to it? is it genuinely
better than an instant cut? Any "no" → make it instant or revise.

---

## 7. Open risks / known defects to fix before porting

- `WorkspaceCubeGridProps.onSelect` collides with React `HTMLAttributes` — rename
  before that component goes near production.
- `PlanStatusBreakdown` spacing: must read `1 todo · 0 review · 0 blocked · 83 done`
  with real gaps and `·` separators on one line; the playground copy still renders
  cramped.
- Micro-size divergence (§0): playground degrades to one breathing dot because
  three 2.5px specks are illegible; production asserts three. My recommendation is
  the single breathing dot at micro (optical sizing — same signature, fewer details
  than the eye can't resolve anyway); if production keeps three, the badge needs to
  grow instead. **Andrey should make this call**, and whichever wins, the
  `ActivityDots.test.tsx:44` assertion must be updated to match the decision.
- Voice: warmup release/lock race and pointer-cancel locking are subtle and easy to
  regress silently; they are documented but not covered by tests.

<!-- revision: 1 -->
