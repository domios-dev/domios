---
tags:
- friction
---

ALL 13-canvas specs (canvas-worlds-v3, substrate-v2-crisp, etc.) skip with 'no active plans in the dev workspace' when a fresh Playwright session lands on an empty board. They depend on the dev workspace already having a plan board open — there's no test-setup that seeds/opens one. This blocks autonomous canvas verification (e.g. substrate-v2 size-constant + crisp eval) without intruding on a shared dev's workspace. Add a beforeAll/helper that creates-or-opens a known plan board (deterministic) so canvas specs are self-sufficient and don't skip.