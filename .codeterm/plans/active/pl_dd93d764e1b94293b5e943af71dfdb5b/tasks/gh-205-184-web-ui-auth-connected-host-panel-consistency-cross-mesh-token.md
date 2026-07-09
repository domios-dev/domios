---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_b27be62d699d48e79765f180792384cf
handle: gh-205-184-web-ui-auth-connected-host-panel-consistency-cross-mesh-token
title: 'GH #205 + #184: Web UI auth — connected-host panel consistency + cross-mesh token propagation'
status: done
---

Two web-auth issues, one lane. STEP 1: gh issue view 205 && gh issue view 184. (A) #205: connected-host panels in the web UI use inconsistent browser auth (some panels authenticated, others not — likely mixed token sources between the local daemon token and per-host tokens). (B) #184: switching to a mesh peer from the web UI fails because the auth token is not propagated to the remote peer's daemon — trace the peer-switch flow (web client -> local daemon -> mesh relay -> peer daemon), find where the Authorization context is dropped, fix so a web session can drive a paired peer. SECURITY correctness: never send the LOCAL daemon's token to a REMOTE peer — the mesh relay must mint/forward the proper peer-scoped credential; read the mesh security audit notes (mem: project_mesh_security_audit) before choosing the mechanism. If either issue is already fixed on branch, close it with evidence.
