# Opus concepts — team activity indicators

Eight compact, self-contained motion concepts for showing a working agent team
in a **dark terminal sidebar**. Each one is a small "what is the team doing right
now" indicator, sized for a ~240–320px column.

## Usage

```tsx
import { OpusConcepts } from './concepts/opus/OpusConcepts';

<OpusConcepts />;
```

The CSS is imported by the component; no setup needed. Named and default exports
both resolve to the same component.

## The eight concepts

| # | Title    | What it shows                          | Motion |
|---|----------|----------------------------------------|--------|
| 1 | Relay    | Task handed off down the agent chain   | Token travels a track, lighting each node on arrival |
| 2 | Sync     | Coordination heartbeat                 | Core beats, concentric rings expand out |
| 3 | Merge    | Parallel lanes converge into one       | Chips run three lanes into a merge point that pulses |
| 4 | Standup  | Turn-taking as each agent reports      | A single raised bar walks across the row |
| 5 | Weave    | Messages crossing between two rows      | Dots cross on diagonals between two peers |
| 6 | Orbit    | Workers circling the orchestrator      | Worker dots orbit a central hub |
| 7 | Pipeline | A task stepping through stages         | Chip steps queued → running → done, lighting each slot |
| 8 | Quorum   | Votes gathering until consensus        | Dots fill one by one, then a check confirms |

## Design intent

- **Palette** — near-black terminal background, cyan for workers, amber for the
  orchestrator / attention, green for completion. Roles are consistent across all
  eight so the vocabulary reads the same everywhere.
- **Signature** — a restrained phosphor glow (`box-shadow`) on the *active*
  element only. It is the one bold move; everything else stays quiet.
- **Type** — a monospace utility face for labels, matching terminal vernacular.

## Constraints honoured

- **CSS-only motion** — every animation uses `transform` / `opacity` (plus color
  swaps); no JS timers, no layout-thrashing properties.
- **No dependencies** — plain React + a single CSS file.
- **`prefers-reduced-motion`** — all animation is disabled and each concept
  freezes on a legible representative frame rather than a blank stage.
- **Accessible** — each stage is a labelled `role="img"` with a plain-language
  description; the moving parts are `aria-hidden`.
- **Self-scoped** — every rule lives under `.opus-concepts`; tokens are declared
  locally so the component looks right even without a host theme.
