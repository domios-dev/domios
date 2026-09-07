#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const [outputDir, collectorUrl, installToken, providerId] = process.argv.slice(2);
if (!outputDir || !collectorUrl || !installToken || !providerId) {
  throw new Error("output directory, collector URL, install token, and provider id are required");
}

fs.mkdirSync(outputDir, { recursive: true });
const installProbe = path.join(outputDir, "install-probe.mjs");
fs.writeFileSync(installProbe, `
const response = await fetch(${JSON.stringify(`${collectorUrl}/install`)}, {
  method: "POST",
  headers: { "content-type": "application/json" },
  body: JSON.stringify({ token: ${JSON.stringify(installToken)}, tab_id: process.env.CODETERM_TAB_ID || "" }),
  signal: AbortSignal.timeout(30000),
});
if (!response.ok) throw new Error("install receiver refused observation");
`, "utf8");

const quote = (value) => `'${value.replaceAll("'", "''")}'`;
const unixQuote = (value) => `'${value.replaceAll("'", `'\\''`)}'`;
const manifest = {
  id: providerId,
  version: "0.1.0",
  displayName: `Provider Setup Fixture ${providerId}`,
  hostApi: 2,
  entry: "plugin.js",
  spawn: {
    readinessStrategy: { kind: "poll_shell_prompt", timeoutSecs: 30 },
    taskDelivery: "inline_flag",
    verifySubmit: { kind: "none" },
  },
  installer: {
    windows: `node ${quote(installProbe)}`,
    macos: `node ${unixQuote(installProbe)}`,
    linux: `node ${unixQuote(installProbe)}`,
  },
};
fs.writeFileSync(path.join(outputDir, "plugin.json"), JSON.stringify(manifest, null, 2), "utf8");

const pluginSource = `
const collectorUrl = ${JSON.stringify(collectorUrl)};
module.exports.default = {
  detectFromTitle() { return false; },
  detectFromOutput(text) { return String(text || "").includes("PROVIDER_SETUP_FIXTURE_READY"); },
  screenHasTui(text) { return String(text || "").includes("PROVIDER_SETUP_FIXTURE_READY"); },
  buildLaunchCommand(params) {
    const task = String((params && params.task) || "");
    const source = \`
const response = await fetch(\${JSON.stringify(collectorUrl + "/configure")}, {
  method: "POST",
  headers: { "content-type": "application/json" },
  body: JSON.stringify({ task: \${JSON.stringify(task)}, tab_id: process.env.CODETERM_TAB_ID || "" }),
  signal: AbortSignal.timeout(30000),
});
if (!response.ok) throw new Error("configure receiver refused observation");
process.stdout.write("PROVIDER_SETUP_FIXTURE_READY\\n");
process.stdin.resume();
\`;
    const script = host.fs.writeTempFile(source, ".mjs");
    if (!script) throw new Error("fixture launch script could not be materialized");
    return "node " + host.shell.quoteFor(script, host.platform());
  },
};
`;
fs.writeFileSync(path.join(outputDir, "plugin.js"), pluginSource.trimStart(), "utf8");
