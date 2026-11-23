# Embeddable live film

The web story plays live on the website as one ESM module plus one stylesheet that the module loads into its own shadow root.

## Build and output

```bash
npx vite build --config playground/vite.film-lib.config.ts
```

Writes `playground/dist-film/` (self-ignored via its own `.gitignore`):

| Path | What |
|---|---|
| `index.js` | Entry. `import { mountFilm } from ".../index.js"` |
| `film.css` | Loaded by the module into the shadow root. Do not link it from the page. Must sit next to `index.js`. |
| `chunks/*.js` | Lazy chunks (lucide icons, yaml). Same origin, next to `index.js`. |
| `assets/*` | Images the product UI references, as real files (no `data:` URLs). |
| `public/` | `web-hale.m4a`, `web-hale.captions.json` and the four fallback clips. Serve them from one folder; pass `audioUrl` and `captionsUrl` into it. |

Fallback clips are named `web-hale.fallback-{landscape|portrait}-{dark|light}.mp4` and are resolved next to `audioUrl`.
Regenerate them after any re-cut of the reel: `node playground/src/film/embed/build/fallback-clips.mjs http://127.0.0.1:<port>/film.html` against a playground server.
A voice-only change (new m4a/captions in `playground/public/film-audio/`) needs only a rebuild.

## Contract

```ts
mountFilm(root, { reel: "web-hale", audioUrl, captionsUrl, theme, layout, reducedMotion })
  -> { setTheme, setLayout, play, pause, setMuted, subscribe, destroy }
```

- `play()` starts without moving focus and never changes the sound; it rejects when the browser refuses sound without user activation, and the film stays unstarted. Calls before the first render wait for it.
- `subscribe(listener)` calls back with `{ started, playing, muted, ended, chapter, pausedBy }` on every change and once right away; `pausedBy` is `"host"` only after `handle.pause()`.
- While playing, a mouse over the stage shows a chip naming the element under it. Scenes opt elements in with `data-film-hint="Title: description"`; the innermost one wins and gets an outline unless it fills most of the stage.
- After the end the final interface stays on screen as a live, hoverable Domios with Replay and chapter chips; seeking or Replay leaves that state.

- Mounts `div.domios-film` (shadow host) inside `root`. Landscape fills the width at 16:9 with a 14px radius; portrait is 9:16, centred, width `min(100%, var(--domios-film-max-height, 100svh) * 9/16)`. Set `--domios-film-max-height` on the slot to cap the portrait height.
- Shows a poster frame (memory graph with title and legend) and a Play button. Nothing in `public/` is requested before Play.
- Player bar: play/pause, time, scrub with chapter ticks, chapter menu, captions toggle. It auto-hides while playing.
- Keys (Space/K, arrows) act only while focus is inside the film; the page keeps scrolling otherwise. Play moves focus into the film.
- `setTheme` repaints live, `setLayout` swaps the stage (playback keeps going), `pause` pauses, `destroy` stops audio and every frame loop and removes the host.

## Workspace (interactive demo)

```ts
mountWorkspace(root, { theme, reducedMotion, layout, place, onPlace, onGuideEnd, tour })
  -> { setTheme, setLayout, setPlace(place), guide(action), setTour(on), anchor("composer"), pause, resume, destroy }
```

- One live Domios workspace built from the app's own components (Titlebar with WebPeerPicker, Sidebar, TabTopBar, MessageBubble tool cards, PlanViewShell, MissionControlTeamBoard, ExtensionsView, JobsView, MemoryListTab, MeshPanel) on sample data (`../workspace/story.ts`). Places: `chat`, `machines`, `plan`, `teams`, `usage`, `plugins`.
- The workspace code is its own chunk, fetched on the first call; the handle works at once.
- The host fills `root` (give it a height; without one it falls back to 16:10, or 9:16 for `portrait`). It renders at the slot's real size: `portrait` is the app's own phone shell, never a scaled desktop.
- One clock drives append-only demo activity across chats and the board. History is bounded. Navigating keeps the clock in place. Reduced motion shows a finished frame.
- The website mounts with `tour: false` and calls `guide(action)` when a visitor selects a shortcut. On desktop the cursor clicks real controls and stops on visitor input. Touch and reduced motion open the same UI directly. The host pauses activity while offscreen.
- Controls the demo does not run answer with the app's own toast, "Not in the demo"; the composer is inert and `aria-hidden`, and `anchor("composer")` gives its rect.
- `onPlace` fires only when a click inside the UI (the visitor's or the tour's) lands in another place.
- Test page: `npx vite preview --config playground/vite.film-lib.config.ts --outDir . --port 5431` → `/workspace-embed.html?theme=light&layout=portrait&place=plan&reduced=1&pauseAt=3`.

## CSP

Runs under `script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self'; media-src 'self'; connect-src 'self'; font-src 'self'`.
No eval, `new Function`, inline scripts, workers, CDNs or `data:` images. Fonts: none are bundled; the film uses the product's system font stacks (Inter/JetBrains Mono first if the page or OS provides them).

## Isolation

- All CSS lives in the shadow root. At build time `:root`, `html` and `body` selectors become `.domios-film-root`; `@property` registrations become a base layer; `rem` becomes px; `vw/vh` become stage percentages; width media queries become `@container` queries on the stage; hover/pointer queries answer as desktop. Every `--ct-*`/`--tw-*` name the film reads is reset on the film root, so page variables never inherit in.
- Product listeners on `document` are wrapped so they see the event's real target inside the shadow root, as they would un-embedded; otherwise every click inside the film reads as outside and closes its menus first.
- The build rewrites the film's own source so `document.documentElement/body`, `window.__*`, `fetch`, `localStorage/sessionStorage`, `innerWidth/innerHeight`, `visualViewport` and `matchMedia` resolve to a film scope (`scope.ts`). The film writes nothing to the page's globals, storage or `<head>`, and its portals stay in the stage.

## Weak devices

During the first 30 frames after Play, the median frame interval is measured. Above 24 ms the memory-graph span (the first four cuts) plays from the small h264 clip for the current layout and theme, synced to the audio clock, and the live scene unmounts. Elsewhere the film stays live. The clip is fetched only after that verdict. `data-fallback="clip|live"` on `.film-box` shows the decision.

## Test page

`playground/film-embed.html` is a page with conflicting `--ct-*`, a 10px root, global resets and its own fonts. It lazy-imports the module on IntersectionObserver and shows isolation evidence. Serve it with the site's CSP:

```bash
npx vite preview --config playground/vite.film-lib.config.ts --outDir .
# http://127.0.0.1:5430/film-embed.html   (?layout=portrait&theme=light&reduced=1)
```
