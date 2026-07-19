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
- Keep repeated ambient motion quiet. Reserve stronger motion for short state transitions.
- Add a useful accessible name to every visual concept.
- Verify the operating-system `prefers-reduced-motion` mode.

## Handoff

Report the concept name, intent, owned files, and verification command. A concept enters production only after it is compared in context and adapted to the application's real theme tokens and status semantics.
