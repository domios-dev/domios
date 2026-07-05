---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: task
id: task_377bdc3aeb0646e3a4c61157185a7c42
handle: r4-perf-root-cause-scale-fixes-20k-cards-46fps-dom-no-webgl
title: 'R4 — Perf root-cause + scale fixes: 20k cards @46fps (DOM, no WebGL)'
status: done
---

Follow-on after S3-S6 (eight/cm-canvas-fix lane). Measured THREE walls: (1) layout packCloud O(N^2) (16k=90s); (2) daemon task-ingestion superlinear (~1500-task 15s abort) - separate owner; (3) renderer per-frame O(N) derive. Fixed client-side: spatial-grid pack O(N^2)->~O(N) (16k 90s->0.63s) + 250k stack-overflow fix (edges.push spread); scene rebuild DECOUPLED from camera scale (killed O(N^2) semanticText per zoom frame); SVG edges culled to visible nodes (world DOM 41k->777). RESULT: 20k complex cards @ 46fps, 2k @ 120fps. Target recalibrated by Andrey 500k->20k, so DOM/xyflow model is sufficient; WebGL parked.
