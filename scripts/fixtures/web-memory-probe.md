# Manual transcript memory probe

Run `pnpm install --frozen-lockfile`, then `node scripts/web-memory-probe.mjs`.
This is an explicit manual browser regression, outside the archived Playwright
suite and automatic gates. It requires an installed Playwright Chromium.

The probe serves an esbuild bundle on a free loopback port above 9000. It starts
no Domios daemon and calls no live API. It feeds the real transcript store
64,000 synthetic assistant/tool rows across eight identities, switches the
mounted transcript subscription, writes heavy output through the real terminal
writer into two xterm instances, and mounts/unmounts real local image/document
previews with a fake bridge. Content is unique 4 KiB text per row.

Every phase awaits xterm write callbacks and a browser frame, then forces GC
before measuring heap. Snapshots are saved at start, phases 4/8/32, and cleanup.
The printed artifact directory also contains `curve.json`, including Chromium
process IDs, Windows private/working-set memory or Unix RSS, and elapsed times.

The regression requires each transcript to retain at most 440 rows, final heap
to stay within 12 MiB of phase 4, no browser exceptions, and cleanup to release
at least half the final heap. `--baseline` disables the retention/growth
assertions to record a leaking implementation; it does not change source.

This focused probe isolates transcript retention. It does not exercise real
chat transport, the complete Chat UI, Blueprint/Jobs views, notifications,
Changes, service workers, or a real web mirror. Canonical live verification
must cover those surfaces before closing a broader browser-memory incident.
