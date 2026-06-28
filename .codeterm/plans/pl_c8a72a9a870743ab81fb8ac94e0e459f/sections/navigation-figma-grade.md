**HOW navigation reaches Figma grade.** A single native non-passive wheel listener in `useCanvasViewport.ts` routes every gesture: plain two-finger scroll pans both axes, cmd/ctrl+scroll (and pinch, which WebKit reports as `ctrlKey`) zooms around the pointer. Non-passive so `preventDefault` kills the page rubber-band:

```ts
const onWheel = (e: WheelEvent) => {
  const rect = el.getBoundingClientRect();
  if (e.ctrlKey || e.metaKey || wheelBehavior === "zoom") {
    e.preventDefault();
    const dy = pixelDelta(e.deltaY, e.deltaMode);   // normalize line/page → px
    zoomAround(Math.exp(-dy * ZOOM_INTENSITY), e.clientX - rect.left, e.clientY - rect.top);
    return;
  }
  // …else pan by (dx, dy)
};
```

`zoomAround` keeps the point under the cursor fixed (the Figma feel) by solving the transform so `px,py` map to themselves at the new scale. The subtle part is the **scroll-trap escape**: a node body owns the vertical wheel while it can still scroll; at its top/bottom boundary the first ticks are *absorbed* (a rubber-band beat) and only the next deliberate scroll breaks free to canvas pan — a momentum-fling tail (gap ≤220ms) won't escape, a fresh gesture will. A sideways wheel always pans (a second exit hatch).

Fly-to is `animateTo`/`animateToView`: an ease-out-cubic rAF loop that drives ref + layer paint + React state together so canvas, overlays, and world DOM travel in lockstep; it honors `prefers-reduced-motion` (lands instantly) and any pan/zoom cancels it instantly (`cancelAnim`) — a fly must yield to the pointer. `fitToView` frames the real content bbox on mount.

## Key decisions
- **One non-passive wheel handler over per-element listeners** — pan/zoom/scroll-trap are decided in one place, so they can't fight each other.
- **Exponential zoom (`Math.exp`) around the pointer** — smooth and scale-invariant; the `220ms` momentum-gap heuristic distinguishes intent from inertia so the board doesn't "fall through" a scrollable node.
- **Fly yields to direct manipulation** — `cancelAnim` on any wheel/drag; a camera animation never wrestles the user.
