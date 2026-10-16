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

When the endpoint contains several DEV pages, append the exact owned CDP target
ID after the viewport argument (`1440x900 TARGET_ID` or `390x844 TARGET_ID`).
The trace refuses an absent or ambiguous selection; do not close other pages
to make it pass. Desktop widths use desktop emulation, phone widths use mobile.

The trace captures 40 animation-frame samples and compositor PNGs. Its JSON
records first correct visible viewport, foreign-visible samples, network byte
volume, and terminal writes after the switch. Run both directions, cold and
warm, recording the health build SHA. Compare an identical viewport and buffer
size before and after. A pass has no foreign frames and a correct visible
viewport; warm retained terminals should receive zero replay bytes. Inspect the
PNGs as well: computed visibility alone cannot prove pixel identity or occlusion.
This is a manual measurement, not an automatic test or readiness wait.

The trace also records write submission/completion, resize duration, long tasks,
terminal dimensions, screen bounds, and horizontally scrolled ancestors. Compare
the first painted screen width with its host to catch a reveal before fitting.
Zero network bytes with a nonzero write means replay came from the browser cache.
Set `TERMINAL_TRACE_PROFILE=1` to save `cpu-profile.json` for a separate diagnostic
capture; profiling and compositor capture add overhead, so retain unprofiled
before/after measurements on identical fixtures too.

For the silent-tab case, attach with an empty buffer. The output stream emits
`{tab_id, pty_id, boot, offset, replay_complete: true}` once its retained replay
has drained, including at offset zero. The client accepts only the subscribed
tab and bound PTY, then queues its first-frame acknowledgement behind pending
terminal writes. Transport activity and keepalives cannot reveal the frame.

The isolated benchmark compares the former full-cache snapshot with a snapshot
from a terminal's known cursor, then verifies a five-byte delta. It measures
local cache-copy cost, not network or terminal-parser speed.
