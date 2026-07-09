---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_ddbeb761e3db483694694a07e1d78384
handle: body-text-must-reveal-on-zoom-worlds-content-band-26
title: Body text must reveal on zoom (worlds content band) §26
status: done
---

BUG + concept gap. Today body is effectively invisible: PLAN node renders index-only (title+count+dot) by design; SPEC body only at a contentNear threshold NOT reached even at 173%; task cards show header only (body/steps/subtask text never reveal on zoom; 'Why this model?' is a click-toggle). Fix: at the WORLDS content band the REAL markdown body must render for plan sections, spec sections, AND task cards — markdown structured/separated, reachable by zoom alone, no toggles. Depth band reveals subtask bodies + milestone/step texts. Part of §26 worlds model.
