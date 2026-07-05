---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_d35d3baaa9f7449a82b308bf31842a5d
handle: worlds-v2
title: WORLDS v2 — birds-eye fill, hierarchy-dim, bounded growth, stride-snake, interactivity §32
status: done
---

# WORLDS v2 implementation — full spec in design doc §32

Implement docs/specs/2026-06-24-canvas-v2-design.md §32 (and the related tasks: canvas-text-selectable, section-to-task-nav, handoff-canvas-render). Spine = orthogonalize 4 channels: SPATIAL-Y=hierarchy, PAINT-band=detail (§26 W0-W3), COLOR=identity, OPACITY=transient-focus-ONLY (content always 1.0).

Deliver all 9 of §32: (32.2) W0 compressed dense content (no hollow giants); (32.3) remove content transparency + done SINKS / in_progress RISES via shelves+sort (~300ms slot anim, no jitter); (32.4) two-axis bounded content-driven size with aspectFloor=0.62W (formula in §32.4); (32.5/32.6) strides = serpentine snake of kind-colored circles, each a leaf micro-world (s0-s3), bloom on zoom — replaces square step blocks; (32.6 text) selectable/copyable text via CSS pointer/select split (no overlay); (32.7) section-click → fly+blink to tasks + W2/W3 go-to-task/spec/open-pane buttons (camera on USER click only §31); (32.8) handoff renderer = orchestrator-region timeline lane of coins (register kind=handoff; checkpoint el_e8ab8c97 to render against). Verify per §32.9 (Playwright: W0 not hollow, content opacity 1.0, done below in_progress by POSITION, size~volume w/ aspectFloor, stride snake, text selectable, section nav, handoff lane, camera byte-identical on live mutation). NO band-aids/legacy/hardcode — structure cleanly.
