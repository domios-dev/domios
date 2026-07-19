# CodeTerm Motion Lab

An isolated visual scratchpad for micro-interactions, status signals, and component motion. It uses the repository's React and Vite toolchain without adding a production dependency or coupling experiments to the application bundle.

## Run

```powershell
npx vite --config playground/vite.config.ts
```

Open `http://127.0.0.1:1431`. Vite watches every playground source file and updates the canvas with HMR.

Read [MOTION_LANGUAGE.md](./MOTION_LANGUAGE.md) before adding a concept. It defines the shared physics, semantic families, and art-director rejection gate.

Build the static playground with:

```powershell
npx vite build --config playground/vite.config.ts
```

## Contribution contract

- Put each exploration family under `playground/src/concepts/<author-or-theme>/`.
- Render concepts in the shared gallery so alternatives remain comparable.
- Use CodeTerm theme semantics through playground custom properties, not fixed product colors.
- Treat [`src/motion-tokens.css`](./src/motion-tokens.css) as the mandatory motion-token source. Concepts must use its duration and easing tokens; do not define local timing values.
- Keep the semantic map from [MOTION_LANGUAGE.md](./MOTION_LANGUAGE.md#semantic-families): Working, Handoff, Completed, and Needs attention have distinct meanings. This gallery currently evaluates Working only.
- Animate only `transform` and `opacity` unless the experiment explicitly measures another property.
- Provide an accessible static meaning and a `prefers-reduced-motion` fallback.
- Keep experimental code out of `src/` until a concept is selected and adapted for production.

## Review gate

The review specification is [MOTION_LANGUAGE.md](./MOTION_LANGUAGE.md#review-protocol). Every promotion requires its deterministic-frame review and the conformance plan in [`tests/motion-lab.spec.ts`](./tests/motion-lab.spec.ts); only an Orchestrator-recorded ACCEPT verdict moves a card from Exploration to Accepted.

## Why a focused Vite lab

Storybook, Ladle, and React Cosmos are strong component-workshop systems. This lab starts smaller because its first job is rapid motion comparison: many variants in one viewport, shared tempo and accent controls, instant HMR, and no new runtime or test dependency. If the lab expands into a general component catalog, its concept modules can be promoted into stories or fixtures later.
