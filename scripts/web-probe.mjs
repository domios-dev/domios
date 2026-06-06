// Manual probe: boot the web mirror in a real browser, pass the token
// gate, and report what rendered. Usage: node scripts/web-probe.mjs
import { chromium } from "@playwright/test";
import { readFileSync } from "fs";
import os from "os";
import path from "path";

const token = readFileSync(
  path.join(os.homedir(), ".codeterm-dev", "config.toml"),
  "utf8",
).match(/^token\s*=\s*"([^"]+)"/m)[1];

const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage();
page.on("console", (m) => {
  if (m.type() === "error") console.log("[console.error]", m.text().slice(0, 200));
});
page.on("pageerror", (e) => console.log("[pageerror]", String(e).slice(0, 300)));

await page.goto("http://127.0.0.1:7686/web/", { waitUntil: "domcontentloaded" });
await page.waitForSelector('[data-testid="web-token-input"]', { timeout: 15000 });
console.log("token gate rendered ✓");
await page.fill('[data-testid="web-token-input"]', token);
await page.click('[data-testid="web-token-submit"]');
await page.waitForSelector('[data-testid="workspace-item"]', { timeout: 20000 });
console.log("app booted: workspace item visible ✓");
const picker = await page.locator('[data-testid="peer-picker-button"]').count();
console.log("peer picker present:", picker > 0 ? "✓" : "✗");
const xterm = await page.locator(".xterm").count();
console.log(".xterm terminals mounted:", xterm);
const errCount = await page.locator("text=/error/i").count();
console.log("visible 'error' texts:", errCount);
await page.screenshot({ path: "test-results/web-probe.png", fullPage: true });
console.log("screenshot: test-results/web-probe.png");
await browser.close();
