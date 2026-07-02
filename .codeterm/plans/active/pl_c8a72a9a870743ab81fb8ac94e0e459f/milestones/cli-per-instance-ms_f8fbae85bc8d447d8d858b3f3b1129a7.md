---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_f8fbae85bc8d447d8d858b3f3b1129a7
handle: cli-per-instance-ms_f8fbae85bc8d447d8d858b3f3b1129a7
title: decision
---

CONFIRMED by BE independently: CODETERM_INSTANCE/CODETERM_API_PORT alone do NOT redirect the CLI — socket-discovery picks a stale/other instance socket unless CODETERM_DATA_DIR or CODETERM_SOCKET_PATH is EXPLICIT. WORKAROUND (works): CODETERM_SOCKET_PATH=/tmp/codeterm-dev-<instance>.sock (main dev = /tmp/codeterm-dev.sock). FIX: socket-discovery must derive the socket path from CODETERM_INSTANCE (and CODETERM_API_PORT) deterministically, so each dev's CLI targets its own daemon without manual CODETERM_SOCKET_PATH.
