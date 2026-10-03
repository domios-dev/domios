# Threshold in motion

Open [index.html](index.html) directly in a browser. It is a self-contained document with inline SVG, CSS and JavaScript; it needs no server, package install, external resource or build.

The prototype uses **original 03 Threshold** at rest. Shape refinement remains a separate decision.

| Study | Behavior | Preview |
| --- | --- | --- |
| Surface tension | A narrow seam opens, overshoots, softens and settles; plays on load, replay, hover or focus | `open.mp4` / `open.gif`, 3 s |
| Come on in | A droplet enters through the cut, merges into the bowl and sends a deformation through its edge | `arrival.mp4` / `arrival.gif`, 3.2 s |
| At home | Subtle expansion and release; loops only after explicit selection or Play | `breathe.mp4` / `breathe.gif`, 4 s |
| A little give | Fast compression, held while pressed, then a damped release with a small overshoot | `press.mp4` / `press.gif`, 3 s |

The SVG stays one connected mark at rest. The entering droplet is a temporary motion element, absorbed into that silhouette. No glow, blur filter, thin ripple ring or animated shadow is needed.

## Motion contract

- The self-contained CSS copies the existing values for `--ct-ease-hover-swell`, `--ct-ease-liquid-settle`, `--ct-ease-liquid-breathe` and the Liquid durations from `web/src/tokens.css`.
- `.ct-liquid-controls` and `.ct-liquid-button` provide the prototype controls. JavaScript reads the actual CSS easing values for arrival, compression and breathing.
- Opening and release use the analytic solution of an underdamped spring: mass 1, stiffness 420, damping 22. Rendering is driven by `requestAnimationFrame`; there are no polling intervals, sleeps or delayed readiness assumptions.
- Pointer press and Space/Enter press support a held compression and release. Blur, pointer cancellation, hidden documents and Pause return to the resting mark.
- `prefers-reduced-motion: reduce` prevents animation, including the initial load effect and control hover/press transforms. Preference changes cancel an active study and restore the exact original path. Replay is disabled while the preference is active.
- Choosing another study cancels the current frame loop. Breathing is opt-in, stops on page hiding, and does not resume automatically.

## Exporting previews

The export tools use Node 22's built-in WebSocket, headless Edge's DevTools protocol, Python/Pillow, FFmpeg and FFprobe. They do not use Playwright or start the app.

From `web/`, supply your own existing Agent Pad destination:

```text
node brand/motion/export-motion.mjs <pad-output-directory>
python brand/motion/encode-motion.py <pad-output-directory>
```

`DOMIOS_EDGE_PATH` can override the browser executable. The default is the installed Windows Edge location. The exporter launches its own hidden headless browser and closes that browser after capture. It never attaches to a running browser or Domios instance.

The browser evaluates the same `at()` motion function used by the interactive prototype at exact frame times. It renders eight frames per atlas and captures them without elapsed-time waits. Media preference inspection waits for the actual `MediaQueryList` change event. The encoder crops those actual browser frames into 640 × 480 / 30 fps H.264 MP4s and 480 × 360 / 20 fps looping GIFs. Every preview must stay below 3,000,000 bytes; the encoder refuses an oversized result and checks video dimensions, duration and decode validity.

The output directory contains the four MP4s, four GIFs, browser screenshots, `capture-manifest.json` and `previews.json`. The manifests record dimensions, byte sizes, reduced-motion states, keyboard/pointer interaction states and spring poses. Rendering atlases stay under ignored `web/brand/.render/round3-motion/`. Existing pad files are never cleared by these scripts.

## Observed results

All four reduced-motion studies remained at the exact original path and identity transform, with the droplet hidden. Pointer and keyboard holds entered compression; release began the spring; Pause restored rest. The sampled compression scale was `1.1 × .88`; release overshot to approximately `.987 × 1.015` before returning to `1 × 1`.

The delivered MP4s are 20,925–28,338 bytes; GIFs are 95,381–199,078 bytes. These previews are saved in the reporting agent's pad, with exact paths in its handoff. The refinements and prototype are proposals, and are not wired into the site.
