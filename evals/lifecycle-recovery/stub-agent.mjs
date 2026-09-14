#!/usr/bin/env node

import { randomUUID } from "node:crypto";
import { createInterface } from "node:readline";

import { writeSessionRecord } from "./session-record.mjs";

const argv = process.argv.slice(2);
const valueAfter = (flag) => {
  const index = argv.indexOf(flag);
  return index >= 0 ? argv[index + 1] : undefined;
};
const provider = valueAfter("--stub-provider") || "claude";
const exitsToShell = argv.includes("--lifecycle-exit-fixture");
const relaunchFixture = argv.includes("--lifecycle-relaunch-fixture");
const api = (
  process.env.CODETERM_API_URL || `http://127.0.0.1:${process.env.CODETERM_API_PORT}`
).replace(/\/$/, "");
const token = process.env.CODETERM_API_TOKEN || "";
const tabId = process.env.CODETERM_TAB_ID || "";
const bindingNonce = process.env.CODETERM_SESSION_BINDING_NONCE || "";
// A resumed launch forwards the provider's own `--resume <session>` argv.
const sessionId = valueAfter("--stub-session") || valueAfter("--resume") || randomUUID();
const connected = Boolean(api && token && tabId);
if (argv.includes("--stub-session-record")) writeSessionRecord(provider, process.cwd(), sessionId);

async function post(path, body, extraHeaders = {}) {
  const response = await fetch(`${api}${path}`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
      ...extraHeaders,
    },
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(10_000),
  });
  if (!response.ok) throw new Error(`${path} failed: ${response.status}`);
}

const reportStop = () =>
  post(`/api/tabs/${tabId}/hook/stop`, { binding_nonce: bindingNonce || undefined });

// Attached before any request, so input written once the session binds is always read.
createInterface({ input: process.stdin }).on("line", async (line) => {
  const text = line.trim();
  if (text === "/exit") process.exit(0);
  if (!text.includes("LR-probe-")) return;
  process.stdout.write(`stub-received ${text}\n`);
  if (connected) await reportStop();
});

process.stdout.write("╭──────────────────────────╮\n│ ❯                        │\n╰──────────────────────────╯\n");

if (connected) {
  await post(`/api/tabs/${tabId}/hook/session_start`, {
    session_id: sessionId,
    source: "startup",
    provider,
    binding_nonce: bindingNonce || undefined,
  });

  if (exitsToShell) {
    await post(
      "/api/plans",
      {
        title: `lifecycle-exit-${tabId}`,
        project_path: process.cwd(),
        owner: { tab_id: tabId, session_id: sessionId, title: process.env.CODETERM_TAB_TITLE || null },
        session_id: sessionId,
        session_binding_nonce: bindingNonce || null,
      },
      { "X-CodeTerm-Tab-Id": tabId },
    );
  }
}

if (exitsToShell || relaunchFixture) process.exit(0);

process.on("SIGINT", () => process.exit(0));
process.on("SIGTERM", () => process.exit(0));
