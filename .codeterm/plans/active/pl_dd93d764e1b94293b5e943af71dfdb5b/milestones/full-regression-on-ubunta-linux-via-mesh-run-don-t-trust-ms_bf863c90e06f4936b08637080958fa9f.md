---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_bf863c90e06f4936b08637080958fa9f
handle: full-regression-on-ubunta-linux-via-mesh-run-don-t-trust-ms_bf863c90e06f4936b08637080958fa9f
title: Linux regression progress
---

named dev instance no longer writes ~/.codeterm/bin; fix integrated in origin/feat/canvas-updates @ 6510578f. ubunta gates: pnpm build PASS, build.sh check PASS on non-prod port, dev startup then exposed CARGO_TARGET_DIR embed path bug (bundle lookup src-tauri/target vs external target). Next bounded lane dispatched to fix bundle-embed CARGO_TARGET_DIR handling before web smoke.
