# CodeTerm motion language

Status: art-direction draft for the Motion Lab.

## Character

CodeTerm motion is a quiet machine at work. It feels precise, lightweight, and
physical without becoming cartoon-like. Persistent status motion stays in the
periphery; it confirms activity without asking for attention.

The visual signature is one anchored structure, one active part, and one brief
reaction at contact. Motion never substitutes for a text or color state.

## Semantic families

| Meaning | Style | Behavior |
| --- | --- | --- |
| Working | Productive | Short action followed by a long rest |
| Handoff | Productive | One directional transfer, then stop |
| Completed | Expressive | One-shot contact and settle |
| Needs attention | Expressive | One-shot interruption; never a perpetual pulse |

The current playground targets **Working** only. Concepts that imply progress,
completion, voting, or synchronization belong in their own semantic families.

## Working-status grammar

- Target box: `24 × 14px`; artwork must remain legible at 1× scale.
- Cycle: `2200–2800ms`; at least 65% of the cycle is visually at rest.
- Primary travel: `4–6px`; rotation at most `16deg`; scale at most `1.06`.
- One primary moving part. One secondary reaction is allowed for at most 120ms.
- Use the active theme accent. Inactive structure stays at 28–36% opacity.
- Glow is an impact artifact, not a persistent material: at most 4px for 100ms.
- Animate `transform` and `opacity`; paint changes are discrete at contact.

Every loop has five authored poses:

1. Rest — a complete, recognizable static glyph.
2. Anticipation — a small movement opposite the main action.
3. Action — the fastest segment, with a clear direction and target.
4. Contact — primary and anchored parts visibly meet.
5. Settle — a smaller reaction returns exactly to rest.

Use productive easing as the default: `cubic-bezier(0.2, 0, 0.38, 0.9)`.
Entrance reactions use `cubic-bezier(0, 0, 0.38, 0.9)` and exits use
`cubic-bezier(0.2, 0, 1, 0.9)`. Linear movement is reserved for indeterminate
progress tracks, never physical objects.

## Rejection gate

Bounce a concept when any answer is yes:

- Does it look like a spinner, screensaver, equalizer, or loading skeleton?
- Does more than one object compete for attention?
- Is there continuous travel with no rest or physical cause?
- Does it bounce, stretch, overshoot, or glow for decoration?
- Does a still frame lose the object, target, or direction of action?
- Does the loop imply a different semantic state than “someone is working”?
- Does it become the brightest object in a normal sidebar?
- Does reduced motion remove the status instead of replacing it with a static cue?

## Review protocol

1. Inspect rest, anticipation, action, contact, and settle as deterministic
   frames, not screenshots captured at arbitrary times.
2. Inspect two complete loops at 1× and 0.75× speed.
3. Place the indicator in the real Team header at 390px and desktop width.
4. Compare every accepted candidate using the same accent and surrounding UI.
5. Keep at most three candidates in the accepted row; everything else remains
   explicitly labelled exploration.
6. Verify keyboard controls, accessible status text, and
   `prefers-reduced-motion: reduce`.

## Sources

- [Walt Disney Animation process](https://www.disneyanimation.com/process/animation/)
- [IBM Carbon motion](https://carbondesignsystem.com/elements/motion/overview/)
- [IBM Carbon choreography](https://carbondesignsystem.com/elements/motion/choreography/)
- [Material duration and easing](https://m1.material.io/motion/duration-easing.html)
- [Apple motion guidance](https://developer.apple.com/design/human-interface-guidelines/motion)
- [W3C reduced-motion technique](https://www.w3.org/WAI/WCAG22/Techniques/css/C39)
