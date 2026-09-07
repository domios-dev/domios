#!/usr/bin/env node

import { randomUUID } from "node:crypto";

const argv = process.argv.slice(2);
const providerIndex = argv.indexOf("--stub-provider");
const provider = providerIndex >= 0 ? argv[providerIndex + 1] : "claude";
const exitsToShell = argv.includes("--lifecycle-exit-fixture");
const api = (
  process.env.CODETERM_API_URL || `http://127.0.0.1:${process.env.CODETERM_API_PORT}`
).replace(/\/$/, "");
const token = process.env.CODETERM_API_TOKEN || "";
const tabId = process.env.CODETERM_TAB_ID || "";

process.stdout.write("╭──────────────────────────╮\n│ ❯                        │\n╰──────────────────────────╯\n");

if (api && token && tabId) {
  const response = await fetch(`${api}/api/tabs/${tabId}/hook/session_start`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      session_id: randomUUID(),
      source: "startup",
      provider,
      binding_nonce: process.env.CODETERM_SESSION_BINDING_NONCE || undefined,
    }),
    signal: AbortSignal.timeout(10_000),
  });
  if (!response.ok) throw new Error(`session_start failed: ${response.status}`);
}

if (exitsToShell) process.exit(0);
process.stdin.resume();
process.on("SIGINT", () => process.exit(0));
process.on("SIGTERM", () => process.exit(0));
