---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: section
id: sec_81bd893081b04915b053dd4ba53c6d0e
handle: substrate-render
title: Substrate & render
---

**HOW we build the render substrate.** The spec asks for zero-lag pan/zoom at 20k nodes, semantic zoom that never hides info, and discrete view modes. None of that survives a DOM-per-node React tree — WebKit repaints the whole visible subtree on every camera commit. So the substrate is a **single `<canvas>` scene graph** with a **per-node bitmap cache**, and the camera is **decoupled from React entirely** during a gesture.

## 1. The camera is a GPU transform, not React state

`useCanvasViewport` paints the camera to a CSS-transformed layer **every** wheel/drag event, but only commits to React state once the gesture *settles*. The layer move is a cheap compositor transform; the React commit (which re-runs culling / LOD / overlays and forces a WebKit repaint) is debounced to gesture-end:

```ts
const schedule = useCallback((next: ViewportTransform) => {
  cancelAnim();
  transformRef.current = next;
  pendingRef.current = next;
  // Paint the camera to the layer EVERY event (cheap GPU transform — smooth pan/zoom).
  paintLayer(layerRef.current, next);
  onPaintRef.current?.(next);
  // Defer the React commit (culling/overlays/LOD) to gesture-END only…
  if (trailRef.current != null) clearTimeout(trailRef.current);
  trailRef.current = window.setTimeout(() => {
    trailRef.current = null;
    setTransformState(transformRef.current);
  }, GESTURE_SETTLE_MS); // 130ms
}, [cancelAnim]);
```

`transformRef` is the live camera, mutated in place and read by the blit loop; `transform` (state) is the *settled* camera that drives the camera-decoupled memos. **Layout/scene memos must never list `cameraScale` in their deps** — that's the load-bearing invariant that keeps re-layout off the frame path.

## 2. Pan = re-blit, not re-shape

Each node is rasterized **once** to an offscreen canvas at its on-screen size × dpr, keyed by a content hash + LOD level. Pan just re-blits cached bitmaps; a card re-rasterizes only when its content, level, or dpr changes — never on scale alone (`ensure`), so a zoom gesture scale-blits the existing bitmap and stays smooth. A separate `resharpen` pass runs at gesture-settle to restore 1:1 crispness once the on-screen size has drifted past tolerance:

```ts
ensure(id, content, w, h, level, dpr, p): CachedBitmap {
  const cur = this.map.get(id);
  if (cur && cur.hash === content.hash && cur.level === level && cur.dpr === dpr) return cur;
  return this.render(id, content, w, h, level, dpr, p);
}
```

`prune(keep)` bounds memory by dropping bitmaps for nodes no longer near the viewport. The scene paint culls to the viewport with a margin before blitting, and **hit-testing is geometric, not DOM** — `hitTestScene` projects each node box and picks the smallest node under the pointer, so there are zero DOM hit targets to cost layout.

## 3. Worlds: LOD keyed off *apparent* width, not camera scale

Semantic zoom ("never hide info") is the `worlds.ts` LOD contract. The band is chosen per node from its **apparent on-screen width** (`footprintWorldWidth × cameraScale`), so a wide SPEC and a small TASK each reveal their interior at the moment they're physically big enough to read — W0 mass → W1 structure → W2 content reveal → W3 deep. Hysteresis at each boundary kills flicker when a node parks near a threshold:

```ts
export function nextWorld(prev, appW, t, h = worldHysteresis): World {
  const raw = rawWorld(appW, t);
  if (!prev) return raw;
  const pi = WORLD_ORDER.indexOf(prev), ri = WORLD_ORDER.indexOf(raw);
  if (Math.abs(ri - pi) !== 1) return raw; // big jump (fit) snaps directly
  const b = Math.min(pi, ri);
  if (appW >= t[b] * (1 + h)) return WORLD_ORDER[b + 1];
  if (appW <  t[b] * (1 - h)) return WORLD_ORDER[b];
  return prev; // inside the dead zone → hold
}
```

## 4. Crisp text at deep zoom without continuous reflow

Because the whole world is one `transform: scale(cam)` layer, a 1× raster blurs at 400%. The fix is to re-rasterize reading-band nodes at on-screen resolution and **quantize the raster scale to 0.25 steps** so the reflow fires a bounded number of times across a sweep, not every frame — and the *same* quantized value drives both the zoom and the counter-transform, so they cancel to exactly 1× world geometry:

```ts
export function crispRasterScale(world: World, cameraScale: number): number {
  if (!atLeast(world, "W2") || cameraScale <= CRISP_MIN_SCALE) return 1;
  return Math.max(1, Math.round(cameraScale * 4) / 4);
}
```

## Key decisions / tradeoffs
- **Canvas over DOM/SVG nodes** — the only way to keep 20k nodes off WebKit's repaint path. Cost: we hand-roll text layout (`cardContent.ts`/`drawCard.ts`) and hit-testing, no free accessibility tree.
- **Settle-debounce (130ms) over rAF-throttle** — committing mid-gesture is the jank; one trailing commit means culling/LOD settle exactly when the camera stops.
- **Discrete view modes** (Document/Card/Tile in `settings.ts`) over continuous counter-scale — continuous text scaling "feels bad"; each mode is a fixed readable size, looking good at its distance.
