# Motion Lab agent contract

This directory is an experimental visual workspace. Root repository instructions still apply.

## Before editing

1. Run the playground and inspect the full comparison grid.
2. Name the single feeling or action the concept must communicate.
3. Work at the enlarged Accepted-card scale before judging the final small size.

## Motion constraints

- The static frame must communicate the status without relying on animation.
- Prefer `transform` and `opacity`; avoid layout-triggering animation properties.
- Reuse `--lab-accent` and `--motion-duration` so every concept responds to the shared controls.
- Start from [`src/motion-tokens.css`](./src/motion-tokens.css), then tune the concept by eye when its physical action needs a distinct rhythm.
- Preserve the semantic map in [MOTION_LANGUAGE.md](./MOTION_LANGUAGE.md#semantic-families): Working, Handoff, Completed, and Needs attention are separate visual languages. This lab's current concepts are Working only.
- Keep repeated ambient motion quiet. Reserve stronger motion for short state transitions.
- Add a useful accessible name to every visual concept.
- Verify the operating-system `prefers-reduced-motion` mode.

## Handoff

Show the concept running at normal speed, slow speed, and final size. The art director accepts it only when the motion reads immediately, feels physical, and is visibly better than the previous version. Automated conformance is not an art-direction substitute.

<!-- revision: 1 -->
