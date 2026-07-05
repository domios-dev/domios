---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_ea183935e4834f93aa4dd83192bc1636
handle: p1b-mesh-homing
title: 'P1b: mesh homing for remote plan mutations'
status: done
---

Mesh homing for plan mutations (remote half). Depends on p1-canonical-sidecar (owning-path model must exist); same rust worker may take it as a follow-on in the same lane.

Scope:
1. Plan record gains owning-peer identity next to canonical_project_path; shown in plan get/state.
2. Auto-homing in the CLI: plan/spec/agent-report commands targeting a plan not owned by the local daemon (lane env CODETERM_PLAN_ID + lane mesh-origin metadata, or plan-id lookup miss with known remote owner) route the mutation over the existing --mesh transport to the owning peer automatically; explicit --mesh always wins.
3. Sidecars materialize ONLY on the owning machine's canonical checkout; the remote worker machine gets no .codeterm/plans writes for foreign plans.
4. Offline owning peer: loud structured failure naming the owning peer (REPORT NOT SENT-style framing for reports); no offline queue in v1.
5. Reads for foreign plans go via the owning daemon; no local sidecar shadow copies.
6. Tests: unit tests for the homing decision (local-owned, remote-owned, explicit --mesh, offline owner); two-peer live smoke IF a second dev peer is reachable (mesh dev pipeline exists — see docs/MESH.md), otherwise record the accepted gap explicitly in the report for the mesh-e2e backlog.

Commands: ./build.sh check; narrowed cargo test; optional two-peer smoke via the mesh dev pipeline.
Report: homing decision matrix implemented, test output, smoke transcript or explicit accepted-gap note.

Auth + auto-context extension (user requirement, spec section worker-context):
7. Every remote plan operation (explicit --mesh or auto-homed) goes through existing mesh peer token auth; audit that the whole plan/spec/report mutation surface is mesh-reachable and NONE of it bypasses auth; unauthenticated attempts fail loudly with the auth reason.
8. Remote lanes carry mesh-origin in lane env/metadata at spawn; plan/spec/report commands in such panes auto-route without the worker typing --mesh (this is the delivery mechanism for the homing decision in step 2).
