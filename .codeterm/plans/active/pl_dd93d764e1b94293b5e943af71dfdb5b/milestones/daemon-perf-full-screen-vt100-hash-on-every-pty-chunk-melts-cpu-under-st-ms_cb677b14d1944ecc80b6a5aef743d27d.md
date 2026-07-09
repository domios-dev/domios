---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_cb677b14d1944ecc80b6a5aef743d27d
handle: daemon-perf-full-screen-vt100-hash-on-every-pty-chunk-melts-cpu-under-st-ms_cb677b14d1944ecc80b6a5aef743d27d
title: audit
---

hidden-work Fable audit confirms ChatComposer delayed-send poll is release-relevant background work; current perf2-composer-poll lane addresses it. Additional hotspots: MeshPanel useMeshData hidden polling, PluginBubbles hidden-pane polling, MemoryPanel hidden pulse renders; PlansContext hidden-pane render storm is larger and should be separate.