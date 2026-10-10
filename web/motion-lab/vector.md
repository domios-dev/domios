# Motion vector: orchard sluices

Three starting words, before drawing anything:

## Sluice

An orchard is many small places fed by connected channels. Water gathers before a gate, stretches through the opening, then settles into the next basin. Translate that into inhabited machine homes, an explicit work handoff, and a returning result. Hold the architecture still; let the work move. A transfer has weight and direction, never an orbit.

## Bookcloth

A bookbinder stretches wet cloth over a board, tucks the edges, and lets it relax. Translate that into section reveals whose leading edge rises first, content following as a single sheet. A tab changes the page beneath a shared, tactile binding. The risk: it explains local organization beautifully but says little about several machines.

## Sealing wax

A warm seal pools, takes an impression, and holds it. Translate that into a button that draws its small trailing bead into its body on hover, compresses on press, then releases a stamped acknowledgement. The risk: a whole site of seals becomes decorative and ceremonious.

## Choice

Choose **orchard sluices** for the system; borrow bookcloth's controlled reveal and sealing wax's small act of joining. A home is a bounded place with inhabitants. The mesh is a set of useful passages between those places. This makes DOM + OS visible without drawing houses, terminals, a central AI brain, or an abstract network of dots.

The visual language is a field notebook: flat ink, quiet open space, numbered plots, bent channels, legible labels. No particles, glowing nets, gradient blobs, typewriter code, or orbiting dots. The Liquid is the behavior of the material, not a layer of effects.

## Liquid inheritance

Study source: app `src/styles/tokens.css`, `globals.css`, and `primitives.css`. Retain `--ct-ease-liquid-settle: cubic-bezier(.16,1,.3,1)` and `--ct-ease-liquid-breathe: cubic-bezier(.42,0,.58,1)`. Keep hover 380ms, press 280ms, segmented 420ms, toggle 380ms, flip 360ms, and sweep 560ms.

Borrow the toggle's physical sequence: normal → 1.32×0.80 stretch → 0.85×1.10 arrival → 1.06×0.96 release → rest. Text sits on a stable layer, so it remains readable while its surrounding material deforms. No linear travel, random idle movement, blur filters, or perpetual full-page painting.

## Four studies

1. **Inhabited mesh:** three machine basins; named agents inside; a build leaves the laptop, reaches the workstation, passes to a server for review, and returns as a result. One complete trip with replay and pause controls.
2. **Waterline:** a narrow seam opens a new section; the heading and its grouped content settle out of that seam. IntersectionObserver owns visibility, never a readiness timer. Content is present without animation.
3. **Common channel:** a shared tab thumb stretches between equal stops; the new panel compresses into place. Caption chips emerge from one stable anchor and settle. Video time supplies captions when integrated.
4. **Gather:** a detached bead joins the action's body on hover or keyboard focus. Press squeezes the material; activation produces a quiet seal. A button still behaves like a button.

All four support light and dark ink, keyboard operation, visible focus, a pause preference, and a completely static reduced-motion mode. Small SVGs plus transform/opacity carry the animation. No motion library.
