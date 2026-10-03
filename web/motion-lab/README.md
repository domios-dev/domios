# Domios Liquid field studies

Independent Vite entry. All authored source stays in this directory; the existing web React/Vite dependencies and three existing product poster images are reused. No motion dependency or external font request.

Direction and rationale: [vector.md](vector.md). Open with `?piece=mesh`, `waterline`, `showcase`, or `gather`; add `&theme=light` or `&theme=dark`. The theme switch and four study links work without query parameters.

## Run

From `web/`:

```powershell
npm ci --ignore-scripts --no-audit --no-fund
node node_modules/vite/bin/vite.js motion-lab --host 127.0.0.1 --port 5184 --strictPort
```

Build and preview, also from `web/`:

```powershell
node node_modules/typescript/bin/tsc -p motion-lab/tsconfig.json
node node_modules/vite/bin/vite.js build motion-lab --outDir dist
node node_modules/vite/bin/vite.js preview motion-lab --host 127.0.0.1 --port 5184 --strictPort
```

The app's ordinary `npm run build` continues to build the production landing page. This entry is deliberately separate. Build output is ignored under `motion-lab/dist/`.

## Integration contract

Merge `lane/web-motion` with an ancestry-preserving merge before adapting selected pieces. No study-page shell, typography, or palette is required for reuse.

| Export | Props | Dependencies | Purpose |
| --- | --- | --- | --- |
| `InhabitedMesh` | `reducedMotion: boolean`, `paused?: boolean` | `useMeshFlow`, mesh styles | Three inhabited machine homes, a build, review, and result. Runs once when visible; replay is explicit. |
| `Waterline` | `children`, `reducedMotion?: boolean` | `useWaterline`, waterline styles | One-shot section seam and grouped content rise at intersection. Content stays present before observation. |
| `CaptionChips` | `captions: readonly string[]`, `activeIndex: number` | `.liquid-captions`, `.liquid-caption`, `.caption-anchor`, `.caption-index`, `caption-pool` | Fixed caption anchor with keyed material entrance. The caller supplies the actual video caption index. |
| `LiquidShowcase` | none | showcase styles and existing posters | Reference tablist with automatic keyboard activation, roving focus, Home/End and arrows. Poster/moment controls demonstrate motion, not playback. |
| `GatherButton` | normal `ButtonHTMLAttributes<HTMLButtonElement>` | `.gather-*`, `bead-gather` | Detached bead joins the CTA; pointer hover and keyboard focus share the effect. For a download use an anchor with the same four spans and `:focus-visible` behavior. |

Common-channel selection bed: animate only the bed, keeping label text on a separate layer. Set `--rail-from`, `--rail-to`, `--rail-count`, and `--rail-index`. For a two-dimensional shelf use measured `translate(x,y)` in place of `translateX`. Preserve the material scale sequence and segment-specific easing in `common-channel`.

Caption indices must be valid for their supplied list. Attach to real `timeupdate`/`seeked` events. Preserve the existing accessible video transcript; decorative caption entrances must not create repeated live announcements.

## Material and timing

`tokens.css` copies the app's existing Liquid timing names and values: hover 380ms, press 280ms, segmented 420ms, toggle 380ms, flip 360ms, sweep 560ms; settle `cubic-bezier(.16,1,.3,1)` and breathe `cubic-bezier(.42,0,.58,1)`. `common-channel` and `bead-gather` inherit the shipped knob's stretch/compress/release sequence. Hero journey duration is illustrative narrative pacing, not readiness or a fake loading state.

Map `--ml-*` colors to the destination palette. Keep material transforms off text wherever possible. The study wordmark is a placeholder, not a proposed final brand mark.

## Accessibility and runtime

- Reduced motion produces a completed, static mesh diagram and removes all animation/transition. Section content remains readable. Interaction still changes state immediately.
- Pause holds the hero at its current position. Other interactions remain readable when selected while paused. Replay and pause controls are disabled under reduced motion.
- Tablist supports ArrowLeft/Right and Home/End, unique ids, controls/label relationships, and one tabbable active tab. All controls have visible focus.
- Hero starts only on intersection and suspends on document visibility/offscreen changes. One `requestAnimationFrame` updates three small SVG parcels; React state changes only on the four narrative phases. No permanent idle loop, filter, canvas, or layout animation.
- Section reveals use `IntersectionObserver`. No timeout, polling, or readiness delay.
- The SVG has a textual title/description covering homes, inhabitants, routes, and result. Tiny illustrative map labels are not its only accessible explanation.

## Inspection and recordings

The tools use headless Edge through the native DevTools protocol, not Playwright. Node 22+ is required. Set `EDGE_PATH` for another Chromium executable and `MOTION_LAB_URL` for another port. Their output directory must be outside the repository; use the owning AgentPad.

```powershell
node motion-lab/tools/inspect.mjs C:/path/to/agent-pad/previews
node motion-lab/tools/record.mjs C:/path/to/agent-pad/recordings
```

Inspection covers every study at desktop/mobile widths in both themes, no horizontal overflow, image decoding, keyboard tab activation/focus, paused interactions, the hero's frame cadence and pause, and all four reduced-motion fallbacks. It saves screenshots and `browser-checks.json`. The `motion-seek` DOM event on the hero SVG is a deterministic recording seam; it freezes and seeks the illustrative timeline. It is not a product protocol.

Recording requires `ffmpeg` on PATH and writes four H.264 MP4s plus `recordings.json`. It deterministically seeks actual browser animations at 30fps, driving pointer and selection events for interactions. Each clip includes light and dark themes; the script refuses a result of 3MB or more. These are browser-rendered previews, not evidence of real-time frame rate; the separate frame sample supplies that observation.

Recordings and visual evidence belong in the AgentPad and the integration report, never committed assets.
