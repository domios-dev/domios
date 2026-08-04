# CodeTerm motion direction

Motion Lab is a visual sketchbook. Its job is to discover movement that feels clear, physical, and specific to CodeTerm. It is not a conformance suite.

## Character

CodeTerm should feel like a small workshop: focused, capable, slightly playful, and never busy for its own sake. A working indicator shows a recognizable action with weight and consequence. It must not read as a spinner, a random twitch, or decorative particles.

## Review loop

1. State one intent in plain language, such as “a hammer lands one measured strike.”
2. Build the motion large enough to see its silhouette, pivot, contact, and settle.
3. Watch several uninterrupted loops at normal speed. Use slow motion only to diagnose a problem.
4. Check the final small size. Simplify the drawing, not the physical idea.
5. Compare the new version beside the previous version in the same card and theme.
6. Accept only when the improvement is immediately visible. Otherwise revise or discard it.

The art director judges rhythm, weight, clarity, personality, and composition. Technical correctness cannot raise a visually weak concept to Accepted.

## Questions that matter

- Can someone identify the object and action without reading its description?
- Does anticipation prepare the main action instead of looking like a glitch?
- Do moving parts visibly meet, react, and settle?
- Is the important action on screen long enough to perceive?
- Does the rest feel intentional rather than dead?
- Do nearby indicators avoid moving in lockstep?
- Does the small version preserve the same idea as the large version?
- Is it more appealing than the version it replaces?

## Minimum product care

- Keep status meaning available as text.
- Provide a useful static state for `prefers-reduced-motion`.
- Prefer transform and opacity for continuous movement.
- Check narrow and desktop layouts manually.
- Keep Exploration honest; do not label an unresolved concept Accepted.

## Reference lineage and production boundary

The current components in `playground/src/ds` and their states in `playground/src/gallery` are the accepted reference lineage. Scratch concepts and gallery filmstrips are comparison evidence; they do not replace or contain that DS/gallery lineage.

- Keep semantic roles, accessible names, pressed states, state coverage, create behavior, and the resolved `PlanStatusBreakdown` tally separator (` · `).
- Adapt accepted motion, tokens, and component behavior to the production component's architecture and constraints.
- Discard scratch scaffolding, filmstrip presentation, lab-only controls, and rejected variants. Production work is an adaptation, not a wholesale playground import.

## Sources

- [Walt Disney Animation process](https://www.disneyanimation.com/process/animation/)
- [IBM Carbon motion](https://carbondesignsystem.com/elements/motion/overview/)
- [Material duration and easing](https://m1.material.io/motion/duration-easing.html)
- [Apple motion guidance](https://developer.apple.com/design/human-interface-guidelines/motion)
- [W3C reduced-motion technique](https://www.w3.org/WAI/WCAG22/Techniques/css/C39)

<!-- revision: 2 -->
