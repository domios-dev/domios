#!/usr/bin/env bash
# Capture mission-control prototype screenshots (detail + overview × 3 variants).
# Requires: dev on 7686 OR run `pnpm dev` in background; playwright chromium installed.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/prototypes/mission-control/screenshots"
mkdir -p "$OUT"

BASE="${CODETERM_PROTOTYPE_URL:-http://127.0.0.1:5173/?prototype=mission-control}"

npx playwright screenshot "$BASE" "$OUT/_boot.png" 2>/dev/null || {
  echo "Start dev first: pnpm dev"
  echo "Or set CODETERM_PROTOTYPE_URL to a running instance"
  exit 1
}

node <<'NODE'
const { chromium } = require("@playwright/test");
const path = require("path");
const out = path.join(process.cwd(), "prototypes/mission-control/screenshots");
const base = process.env.CODETERM_PROTOTYPE_URL || "http://127.0.0.1:5173/?prototype=mission-control";
const variants = [
  ["hangar", "A-hangar"],
  ["control-room", "B-control-room"],
  ["assembly-line", "C-assembly-line"],
];
(async () => {
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  await page.goto(base, { waitUntil: "networkidle" });
  for (const [variant, label] of variants) {
    await page.getByRole("tab", { name: new RegExp(label.split("-").slice(1).join(" "), "i") }).click();
    for (const lod of ["detail", "overview"]) {
      await page.getByRole("tab", { name: lod === "detail" ? "100% LOD" : "Zoomed-out LOD" }).click();
      await page.waitForTimeout(400);
      await page.locator(".mc-stage").screenshot({ path: path.join(out, `${label}-${lod}.png`) });
    }
  }
  await browser.close();
  console.log("Wrote screenshots to", out);
})();
NODE
