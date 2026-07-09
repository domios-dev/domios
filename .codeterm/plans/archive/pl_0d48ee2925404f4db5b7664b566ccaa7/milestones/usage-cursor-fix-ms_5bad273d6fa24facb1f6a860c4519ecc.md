---
plan_id: pl_0d48ee2925404f4db5b7664b566ccaa7
kind: milestone
id: ms_5bad273d6fa24facb1f6a860c4519ecc
handle: usage-cursor-fix-ms_5bad273d6fa24facb1f6a860c4519ecc
title: decision
---

cursor has NO TUI /usage screen (parseUsage returns null). Real source = api2.cursor.sh Connect-RPC GetAggregatedUsageEvents (spend cents) + GetMonthlyInvoice (cycle reset), keychain bearer token. Spend-based, NO percentage, NO plan limit. Needs non-TUI fetch capability + snapshot spend fields.
