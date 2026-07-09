---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_1a208160fef849679e52efc7c6e8f800
handle: bug-stabilize-canvas-layout-and-camera-when-tasks-are-added
title: 'BUG: stabilize canvas layout and camera when tasks are added'
status: done
tags:
- canvas
- layout
- stability
---

When plan tasks are added or updated, the canvas cloud structure shifts too much and the camera visibly jumps/jitters. Stabilize layout identity/order/bounds so incremental plan mutations do not cause large unrelated node movement or camera shake.
