---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: section
id: sec_dc68c552dc754043839a6a168665346c
handle: wave-26-perceived-responsiveness-busy-states-lock-hygiene
title: 'Wave 26 — Perceived responsiveness: busy states + lock hygiene'
---

Round-20: every wait in the product shows a live busy state (the chat typing-dots grammar, one shared system — no dead/blank/static surfaces ever); the daemon emits truthful boot-stage events instead of the UI guessing; Rust lock hygiene becomes mechanical (clippy await_holding_lock enabled and enforced, hold-time instrumentation in dev); web UI and native UI behave identically. DRY, no per-surface bespoke spinners, no legacy shims — pre-prod freedom, redesign over patch.
