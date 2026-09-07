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
} else if (mode === "no-tab") {
  const tabs = Array.isArray(json) ? json : json?.tabs || [];
  if (tabs.some((tab) => tab.id === values[0])) fail("owned fixture tab remains visible");
} else if (mode === "no-provider") {
  const providers = Array.isArray(json) ? json : json?.providers || [];
  if (providers.some((provider) => provider.id === values[0])) fail("owned fixture provider remains installed");
} else if (mode === "no-workspace") {
  const workspaces = Array.isArray(json) ? json : json?.workspaces || [];
  if (workspaces.some((workspace) => workspace.id === values[0] || workspace.workspace_id === values[0])) {
    fail("owned fixture workspace remains visible");
  }
} else if (mode === "no-workspace-name") {
  const workspaces = Array.isArray(json) ? json : json?.workspaces || [];
  if (workspaces.some((workspace) => workspace.name === values[0] || workspace.title === values[0])) {
    fail("run-scoped fixture workspace remains visible");
  }
} else if (mode === "no-workspace-cwd") {
  const workspaces = Array.isArray(json) ? json : json?.workspaces || [];
  const expected = values[0].replaceAll("\\", "/").toLowerCase();
  if (workspaces.some((workspace) => String(workspace.cwd || "").replaceAll("\\", "/").toLowerCase() === expected)) {
    fail("run-scoped fixture workspace cwd remains visible");
  }
} else if (mode === "workspace-created") {
  const actualCwd = String(json?.cwd || "").replaceAll("\\", "/").toLowerCase();
  const expectedCwd = values[1].replaceAll("\\", "/").toLowerCase();
  if (!json?.id || json?.name !== values[0] || actualCwd !== expectedCwd) {
    fail("workspace response does not prove the requested unique name and cwd");
  }
  process.stdout.write(json.id);
} else if (mode === "config") {
  const value = json?.value ?? json;
  process.stdout.write(typeof value === "string" ? value : JSON.stringify(value));
} else if (mode === "config-equals") {
  const value = json?.value ?? json;
  if (value !== values[0]) fail("General Agent provider was not restored");
} else if (mode === "receipt64") {
  const decoded = JSON.parse(Buffer.from(input.trim(), "base64url").toString("utf8"));
  if (decoded?.token !== values[0] && decoded?.task !== values[0]) fail("receiver did not observe the supplied opaque token");
  if (!decoded?.tab_id) fail("receiver observation has no originating tab id");
  process.stdout.write(decoded.tab_id);
} else {
  fail(`unknown assertion mode: ${mode}`);
}
