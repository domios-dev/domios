---
plan_id: pl_0d48ee2925404f4db5b7664b566ccaa7
kind: milestone
id: ms_ff626f9e67444ec6b8d71e7a03efb025
handle: usage-all-providers-ms_ff626f9e67444ec6b8d71e7a03efb025
title: decision
---

ANDREY (separate effort): usage panel must (1) auto-refresh/proactively reset on timer expiry, (2) request usage for ALL providers, (3) fix cursor parse (wrong now). Separate plan + dedicated worker. Architecture already per-provider (usage_parser/probe + plugin parseUsage); panel just shows Claude today.
