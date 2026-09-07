#!/usr/bin/env node

import { spawn } from "node:child_process";

const [apiUrl, apiToken, marker, evalLib, tabId, sessionId, bindingNonce, planId] = process.argv.slice(2);
if ([apiUrl, apiToken, marker, evalLib, tabId, sessionId, bindingNonce, planId].some((value) => !value)) {
  throw new Error("event report requires dev connection, reporter identity, marker, and Plan id");
}

const deadline = AbortSignal.timeout(20_000);
const eventResponse = await fetch(`${apiUrl.replace(/\/$/, "")}/api/events/stream`, {
  headers: { Authorization: `Bearer ${apiToken}` },
  signal: deadline,
});
if (!eventResponse.ok || !eventResponse.body) {
  throw new Error(`event stream refused readiness: ${eventResponse.status}`);
}

const command = `
source "$REPORT_EVAL_LIB"
unset CODETERM_PLAN_ID CODETERM_TASK_ID
CODETERM_SESSION_ID="$REPORT_SESSION_ID" CODETERM_SESSION_BINDING_NONCE="$REPORT_BINDING_NONCE" \
  dc --as-tab "$REPORT_TAB_ID" agent report "$REPORT_MARKER" --plan "$REPORT_PLAN_ID" --json
`;
const child = spawn("bash", ["-lc", command], {
  env: {
    ...process.env,
    REPORT_EVAL_LIB: evalLib,
    REPORT_TAB_ID: tabId,
    REPORT_SESSION_ID: sessionId,
    REPORT_BINDING_NONCE: bindingNonce,
    REPORT_MARKER: marker,
    REPORT_PLAN_ID: planId,
  },
  signal: deadline,
  windowsHide: true,
});

const commandResult = new Promise((resolve, reject) => {
  let stdout = "";
  let stderr = "";
  child.stdout.setEncoding("utf8").on("data", (chunk) => { stdout += chunk; });
  child.stderr.setEncoding("utf8").on("data", (chunk) => { stderr += chunk; });
  child.once("error", reject);
  child.once("close", (code) => {
    if (code === 0) resolve(stdout);
    else reject(new Error(`report command exited ${code}: ${stderr.trim()}`));
  });
});

const eventResult = async () => {
  const reader = eventResponse.body.getReader();
  const decoder = new TextDecoder();
  let buffered = "";
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) throw new Error("event stream ended before the matching report");
      buffered += decoder.decode(value, { stream: true }).replaceAll("\r\n", "\n");
      const frames = buffered.split("\n\n");
      buffered = frames.pop() ?? "";
      for (const frame of frames) {
        const payload = frame.split("\n")
          .filter((line) => line.startsWith("data: "))
          .map((line) => line.slice(6))
          .join("\n");
        if (!payload) continue;
        const event = JSON.parse(payload);
        if (event?.event_type === "agent-report" && JSON.stringify(event.data).includes(marker)) {
          return event;
        }
      }
    }
  } finally {
    await reader.cancel().catch(() => {});
  }
};

const [stdout] = await Promise.all([commandResult, eventResult()]);
process.stdout.write(stdout);
