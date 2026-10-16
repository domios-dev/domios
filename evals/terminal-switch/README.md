# Terminal switch evidence

Use an owned Domios Browser on DEV with two shell tabs whose buffers contain
distinct markers. Generate at least 18,000 recognizable lines in each shell and
an exact final marker (`FRAME_A_READY`, `FRAME_B_READY`). Create and close these
tabs in a dedicated workspace through the `dc` wrapper from `evals/_lib/lib.sh`.
Authenticate without printing the token. Do not use the owner's Browser target.

Get the owned Browser endpoint with `codeterm plugin browser endpoint`, then run:

```bash
node evals/terminal-switch/trace.mjs <endpoint> PoisonB FRAME_B_READY FRAME_A_READY <artifact-directory>
node evals/terminal-switch/trace.mjs <endpoint> PoisonA FRAME_A_READY FRAME_B_READY <phone-artifact-directory> 390x844
pnpm exec tsx evals/terminal-switch/replay-bench.ts
```

The trace captures 40 animation-frame samples and compositor PNGs. Its JSON
records first correct visible viewport, foreign-visible samples, network byte
volume, and terminal writes after the switch. Run both directions, cold and
warm, recording the health build SHA. Compare an identical viewport and buffer
size before and after. A pass has no foreign frames and a correct visible
viewport; warm retained terminals should receive zero replay bytes. Inspect the
PNGs as well: computed visibility alone cannot prove pixel identity or occlusion.
This is a manual measurement, not an automatic test or readiness wait.

The isolated benchmark compares the former full-cache snapshot with a snapshot
from a terminal's known cursor, then verifies a five-byte delta. It measures
local cache-copy cost, not network or terminal-parser speed.
