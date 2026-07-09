---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_de0652702d564e629dd39da689740cd7
handle: first-run-general-agent-selection-screen-with-install-fallback
title: 'First run: General agent selection screen with install fallback'
status: done
---

## What

First launch with NO main/General tab: instead of silently doing nothing (or failing), spawn an empty tab with a "Choose your General agent" screen.

## Behavior

- Detection: on startup, no main/general pane exists -> open the first-run tab automatically.
- Screen: same picker visual language as New-session, headline "Choose your General agent". Provider cards with INSTALLED detection (existing provider availability probes): claude preselected if installed; else the first installed provider preselected; if NONE installed -> instruction cards per provider (install command / link, claude first) with a re-check button.
- Confirm -> spawns the General Agent (main tab, protected close rule applies as today).
- This screen must never block the rest of the app: user can still open plain terminal tabs from the sidebar.

## Files

FirstRunGeneralPicker (shares picker primitives with NewSessionPicker — one component family); startup detection wiring; provider availability probe reuse.
