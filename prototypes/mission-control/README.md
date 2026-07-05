# Canvas mission control — concept prototype

User-gate artifact for plan operations room layout. **Not wired to live Canvas.**

## Run (interactive React gallery)

From repo root, with dev server or Vite alone:

```bash
pnpm dev --open "/?prototype=mission-control"
```

Or open in an already-running dev instance:

```
http://127.0.0.1:7686/?prototype=mission-control
```

Toggle **A · Assembly line**, **B · Hangar floor**, **C · Control room**, and **100% LOD** vs **Zoomed-out LOD** in the toolbar.

## Static artifact (no dev server)

Open in any browser:

```
prototypes/mission-control/index.html
```

Or:

```bash
open prototypes/mission-control/index.html
```

## Source module

```
src/components/Canvas/missionControl/
```

Entry: `MissionControlPrototype.tsx` · seeded data: `fixtures.ts`

## Motion spec (for live build)

| Transition | Duration | Easing | Notes |
|------------|----------|--------|-------|
| Task card enters bay / dispatch rail | 320ms | `--ct-ease-out-expo` | `mc-card-in`: fade + 6px rise |
| Agent avatar docks to lane | 280ms | `--ct-ease-out-expo` | translate from dispatch rail; scale 0.92→1 |
| Avatar working pulse | 2s loop | ease-in-out | subtle scale 1→1.06 while `in_progress` |
| Event particle → Plan HUD | 600ms | `--ct-ease-out-expo` | dot flies from station coords to HUD band; opacity 1→0 |
| Gate desk attention | 2.4s loop | ease-in-out | inset glow on `in_review` queue |
| LOD switch (detail ↔ overview) | 320ms | `--ct-ease-out-expo` | stage scale 1↔0.52; `.mc-detail-only` hidden in overview |
| HUD metric tick | 400ms | `--ct-ease-out-expo` | progress bar width only; counts snap |

Particles originate at the **avatar station** (lane bay, QA slot, gate desk) and terminate at the **Plan HUD** metric row — never at arbitrary canvas coordinates.

## User gate question

**Approved default Canvas mission control shell: A · Assembly line.**

- **A · Assembly line** — strict left→right pipeline: dispatch rail -> work floor -> QA bay + gate desk -> done shelf
- **B · Hangar floor** — dispatch left, wide 2×2 work floor, QA/gate/done stack right
- **C · Control room** — central ops ring with flanking dispatch + gate/done; useful contrast, not the selected direction

Refinement target: make unhealthy states physically obvious, especially an empty work floor with non-empty dispatch.

<!-- revision: 1 -->
