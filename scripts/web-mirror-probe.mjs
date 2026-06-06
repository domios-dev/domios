// Live verification of the three mirror fixes: chat history in web,
// agent spawn from web, new-workspace from web.
import { chromium } from "@playwright/test";
import { readFileSync } from "fs";
import os from "os";
import path from "path";

const token = readFileSync(
  path.join(os.homedir(), ".codeterm-dev", "config.toml"),
  "utf8",
).match(/^token\s*=\s*"([^"]+)"/m)[1];
const api = async (method, p, body) => {
  const r = await fetch(`http://127.0.0.1:7686/api${p}`, {
    method,
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await r.text();
  if (!r.ok) throw new Error(`${p} -> ${r.status}: ${text.slice(0, 200)}`);
  try { return JSON.parse(text); } catch { return text; }
};

// 1. chat-raw/start returns real history for the GA pane.
const panes = await api("GET", "/panes");
const ga = panes.find((p) => p.agent_type === "claude" && p.protected);
if (!ga) throw new Error("no GA claude pane found");
const chat = await api("POST", `/panes/${ga.id}/chat-raw/start`);
console.log(`1. chat-raw/start: text=${(chat.text ?? "").length} bytes, start_offset=${chat.start_offset} ${chat.text?.length > 100 ? "✓" : "✗ EMPTY"}`);
await api("POST", `/panes/${ga.id}/chat-raw/stop`);

// 2+3 in the browser.
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage();
page.on("pageerror", (e) => console.log("[pageerror]", String(e).slice(0, 200)));
await page.goto(`http://127.0.0.1:7686/web/#token=${token}`, { waitUntil: "domcontentloaded" });
await page.waitForSelector('[data-testid="peer-picker-button"]', { timeout: 30000 });
console.log("2. booted via share link (no manual gate) ✓");

// New workspace via the actual sidebar button + prompt dialog.
page.on("dialog", (d) => d.accept("C:\\Users\\dev\\.codeterm-dev\\pw-ws-probe"));
const wsBefore = (await api("GET", "/workspaces")).length;
await page.locator('button[title="New workspace"]').click();
await page.waitForTimeout(2500);
const wsList = await api("GET", "/workspaces");
const probeWs = wsList.find((w) => w.name === "pw-ws-probe");
console.log(`3. new workspace via UI button: ${wsBefore} -> ${wsList.length} ${probeWs ? "✓" : "✗"}`);

// Agent spawn through the same server-side path the web button now uses.
const spawned = await api("POST", "/agents/spawn", {
  agent: "claude",
  model: "sonnet",
  workspace_id: probeWs?.id,
  skip_permissions: true,
  activate: true,
  task: "Reply with the word MIRRORPROBE",
});
await page.waitForTimeout(3000);
const panes2 = await api("GET", "/panes");
const agentPane = panes2.find((p) => p.id === spawned.pane_id);
console.log(`4. agent pane spawned from web path: agent_type=${agentPane?.agent_type} ${agentPane?.agent_type === "claude" ? "✓" : "✗"}`);

// Cleanup.
await api("POST", `/panes/${spawned.pane_id}/close`).catch(() => {});
if (probeWs) await api("POST", `/workspaces/${probeWs.id}/close`).catch(() => {});
await page.screenshot({ path: "test-results/web-mirror-probe.png" });
await browser.close();
console.log("probe complete; screenshot: test-results/web-mirror-probe.png");
