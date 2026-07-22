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
- Start with [`src/motion-tokens.css`](./src/motion-tokens.css), then tune a concept when its physical action calls for a distinct rhythm.
- Keep the semantic map from [MOTION_LANGUAGE.md](./MOTION_LANGUAGE.md#semantic-families): Working, Handoff, Completed, and Needs attention have distinct meanings. This gallery currently evaluates Working only.
- Animate only `transform` and `opacity` unless the experiment explicitly measures another property.
- Provide an accessible static meaning and a `prefers-reduced-motion` fallback.
- Keep experimental code out of `src/` until a concept is selected and adapted for production.

## Art-direction loop

The lightweight review loop is in [MOTION_LANGUAGE.md](./MOTION_LANGUAGE.md#review-loop). Develop large, watch the complete motion at normal and slow speed, verify the final small size, and compare it directly with the previous version. A concept moves to Accepted only when the art director can describe the visible improvement without referring to a checklist or automated test.

## Why a focused Vite lab

Storybook, Ladle, and React Cosmos are strong component-workshop systems. This lab starts smaller because its first job is rapid motion comparison: many variants in one viewport, shared tempo and accent controls, instant HMR, and no new runtime or test dependency. If the lab expands into a general component catalog, its concept modules can be promoted into stories or fixtures later.

<!-- revision: 1 -->
