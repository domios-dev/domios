#!/usr/bin/env node

const [mode, ...values] = process.argv.slice(2);
const input = await new Promise((resolve) => {
  let body = "";
  process.stdin.setEncoding("utf8");
  process.stdin.on("data", (chunk) => body += chunk);
  process.stdin.on("end", () => resolve(body));
});
const json = mode === "receipt64" ? null : (input.trim() ? JSON.parse(input) : null);
const fail = (message) => { throw new Error(message); };

if (mode === "health-sha") {
  if (json?.build_sha !== values[0]) fail(`build SHA mismatch: ${json?.build_sha || "missing"}`);
} else if (mode === "hook") {
  const current = json?.hook_contract_current === true && json?.session_start_hook === true && json?.stop_hook === true;
  const actionable = json?.hooks_installed === false && json?.repair_requires_consent === true && json?.integration_state !== "installed";
  if (!current && !actionable) fail("Codex status is neither a full owned contract nor an actionable consent-required refusal");
} else if (mode === "receipt") {
  if (json?.token !== values[0] && json?.task !== values[0]) fail("receiver did not observe the supplied opaque token");
  if (!json?.tab_id) fail("receiver observation has no originating tab id");
} else if (mode === "spawn") {
  if (json?.agent !== values[0] || json?.delivery_state !== "confirmed" || !json?.tab_id) {
    fail("spawn did not confirm the configured fixture provider delivery");
  }
} else if (mode === "install") {
  if (json?.launch_state !== "command_dispatched" || !json?.tab_id) fail("installer route did not acknowledge command dispatch");
} else if (mode === "no-title") {
  const tabs = Array.isArray(json) ? json : json?.tabs || [];
  if (tabs.some((tab) => tab.title === values[0])) fail("rolled-back provisional tab remains visible");
} else if (mode === "config") {
  const value = json?.value ?? json;
  process.stdout.write(typeof value === "string" ? value : JSON.stringify(value));
} else if (mode === "receipt64") {
  const decoded = JSON.parse(Buffer.from(input.trim(), "base64url").toString("utf8"));
  if (decoded?.token !== values[0] && decoded?.task !== values[0]) fail("receiver did not observe the supplied opaque token");
  if (!decoded?.tab_id) fail("receiver observation has no originating tab id");
  process.stdout.write(decoded.tab_id);
} else {
  fail(`unknown assertion mode: ${mode}`);
}
