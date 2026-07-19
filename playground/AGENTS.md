# Motion Lab agent contract

This directory is an experimental visual workspace. Root repository instructions still apply.

## Before editing

1. Run the playground and inspect the full comparison grid.
2. Choose a new concept name and a dedicated folder under `src/concepts/`.
3. Keep ownership isolated to that folder until the concept is ready for the shared gallery.

## Motion constraints

- The static frame must communicate the status without relying on animation.
- Prefer `transform` and `opacity`; avoid layout-triggering animation properties.
- Reuse `--lab-accent` and `--motion-duration` so every concept responds to the shared controls.
- Use [`src/motion-tokens.css`](./src/motion-tokens.css) as the mandatory source for motion durations and easing. Do not introduce local timing or easing values.
- Preserve the semantic map in [MOTION_LANGUAGE.md](./MOTION_LANGUAGE.md#semantic-families): Working, Handoff, Completed, and Needs attention are separate visual languages. This lab's current concepts are Working only.
- Keep repeated ambient motion quiet. Reserve stronger motion for short state transitions.
- Add a useful accessible name to every visual concept.
- Verify the operating-system `prefers-reduced-motion` mode.

## Handoff

Report the concept name, intent, owned files, and verification command. The review gate is the [MOTION_LANGUAGE.md review protocol](./MOTION_LANGUAGE.md#review-protocol) plus [`tests/motion-lab.spec.ts`](./tests/motion-lab.spec.ts). A card moves to Accepted only after the Orchestrator records an ACCEPT verdict from that review; production integration remains a separate adaptation step.
