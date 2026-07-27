# Seeded Canvas World States

The fixtures in `world-states.ts` set `canvas.perfFixtureProfile` and open an
inline workspace canvas with deterministic app-side plans and tabs. Use these
for UI tests that only need stable canvas state, not real daemon-created plans.

Available states:

- `plan-with-known-tasks`: one plan with stable pending, in-progress, review,
  and done tasks. Best default for board/list/canvas controls.
- `tab-tree-with-statuses`: the known-task plan plus deterministic
  orchestrator/worker tabs with Working, Waiting, and Idle agent states.
- `mid-stream-chat-transcript`: a plan bound to a chat-view tab whose assistant
  state is mid-streaming.
- `done-heavy`: existing done-heavy canvas profile with 200 done tasks and 10
  active tasks for rollup and compressed-shelf assertions.

Example:

```ts
import { seedWorldStateAndOpenInlineCanvas, worldStates } from "../../fixtures/world-states";

const canvas = await seedWorldStateAndOpenInlineCanvas(
  webBrowser,
  api as unknown as ApiClient,
  worldStates.planWithKnownTasks,
);
```

