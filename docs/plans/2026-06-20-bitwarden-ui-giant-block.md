# Bitwarden plugin UI — giant empty connection block

> From Andrey 2026-06-20 (2 screenshots): the bitwarden plugin settings view balloons —
> the CONNECTION card (Server input + "Checking status…"/"Unlocked") sits at the top, then
> a ~1000px EMPTY scrollable block below it. Looks like cruft. Make it content-sized.

## Diagnosis (do first — confirm the side)
- Bitwarden's own reserve is small: `bitwarden/ui/src/main.tsx:132` `minHeight: 132` — NOT
  the ~1000px block. So the giant empty area is almost certainly the **HOST plugin-UI
  iframe** (codeterm-alpha) given a tall fixed/`100%` height — bitwarden's small content
  fills the top, the rest is empty + scrollable. Confirm by inspecting the host plugin-UI
  iframe sizing (src/components/Extensions — PluginDetailPanel / whatever renders the
  plugin `view` iframe) AND the bitwarden root container (`main.tsx` root div height/flex).
- The plughard worker earlier "reserved height for stable panel" — check it didn't over-set.

## Fix — content-sized, no giant empty block
- **If host iframe (most likely):** size the plugin-UI iframe to its CONTENT, not a giant
  fixed/`100%` height. Implement an auto-height handshake (plugin posts its content height
  via postMessage; host sets the iframe height to it, with a sane max + scroll only when
  content truly exceeds), or `height: fit-content`/measured-content. This benefits EVERY
  plugin UI (no empty blocks anywhere), with bitwarden as the repro.
- **If bitwarden root contributes:** root must be content-sized (no `height:100%`/flex-grow
  into a tall iframe); keep a small `minHeight` only to avoid the status-land jump.
- Net: the connection card fits Server + status badge + (locked) form; NO 1000px empty
  scroll area; no jump when the deferred status lands (keep the existing deferred-probe
  pattern — just don't reserve/grow huge).

## Gates / verify
- Host: tsc + test:unit. Bitwarden (if touched): rebuild bundle + npm test (codeterm-plugins).
- Visual: bitwarden settings view has no giant empty block in both "Checking status…" and
  "Unlocked" states; other plugin UIs (git, lmstudio) still render correctly (no clipping).
- ROG can visually confirm on Windows.

## Task
- **BW-UI (frontend, Opus):** diagnose host-iframe vs bitwarden-root; fix the giant empty
  block content-sized (prefer the host iframe auto-height — helps all plugins); verify
  bitwarden + another plugin UI render cleanly. If the fix lands in codeterm-plugins/
  bitwarden, make it there + rebuild the bundle (separate repo).

<!-- revision: 1 -->
