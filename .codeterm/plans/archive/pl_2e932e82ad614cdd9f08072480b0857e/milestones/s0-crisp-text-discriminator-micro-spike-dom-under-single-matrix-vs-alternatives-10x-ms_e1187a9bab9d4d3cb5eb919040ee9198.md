---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: milestone
id: ms_e1187a9bab9d4d3cb5eb919040ee9198
handle: s0-crisp-text-discriminator-micro-spike-dom-under-single-matrix-vs-alternatives-10x-ms_e1187a9bab9d4d3cb5eb919040ee9198
title: testing
---

ACCEPTED with pixel+metric evidence. VERDICT: Branch A. Single-container matrix() keeps DOM text crisp at 10x on BOTH Chromium AND WebKit (macOS Tauri engine). var-Laplacian A=376.8 vs re-layout ground-truth B=387.9 (within 3%); R2 predicted ~100x drop, reality ~3% = R2 empirically wrong for idle/at-rest. 0.0% reflow. crispScale buys nothing for idle nodes (it even rewraps text). CAVEAT (real grain of R2): mid-gesture/animation softness only, snaps crisp at rest — does NOT affect idle nodes; likely the original crispScale motivation. DECISION: drop crispScale from idle path, pure DOM under single matrix, NO GPU/MSDF (Branch B not justified). Deliverable spikes/crisp-text/ @8a8c85f9 on canvas/s0-crisp-discriminator (harness+capture.mjs+FINDINGS+screenshots both engines).
