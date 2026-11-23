// Renders the weak-device fallback clips: the reel's memory-graph span, per layout and theme, as small h264 files.
// usage: node playground/src/film/embed/build/fallback-clips.mjs <film-page-url> [outDir=playground/public/film-audio]
// <film-page-url> serves playground/film.html (for example the capture server); run again whenever the reel is re-cut.
import { chromium } from "@playwright/test";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const REEL = "web-hale";
const MEMORY_GRAPH_CUTS = ["lab-memory-hook", "lab-memory-origin", "memory-graph", "lab-dreaming"];
const LAYOUTS = { landscape: { w: 1920, h: 1080, scale: "960:540" }, portrait: { w: 1080, h: 1920, scale: "540:960" } };
const [, , base, outArg] = process.argv;
if (!base) throw new Error("usage: fallback-clips.mjs <film-page-url> [outDir]");
const outDir = resolve(outArg ?? "playground/public/film-audio");
const repo = resolve(import.meta.dirname, "../../../../..");

const browser = await chromium.launch(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : {});
const page = await browser.newPage();
await page.goto(`${base}?reel=${REEL}`, { waitUntil: "networkidle" });
const cuts = await page.evaluate((id) => window.__filmCuts?.[id] ?? [], REEL);
await browser.close();
const first = cuts.findIndex((c) => MEMORY_GRAPH_CUTS.includes(c.id));
if (first < 0) throw new Error(`no memory-graph cut in ${REEL}`);
let last = first;
while (cuts[last + 1] && MEMORY_GRAPH_CUTS.includes(cuts[last + 1].id)) last++;
const from = cuts[first].start;
const to = cuts[last].end;
console.log(`memory span ${from}..${to}s (${cuts.slice(first, last + 1).map((c) => c.id).join(", ")})`);

const work = mkdtempSync(join(tmpdir(), "film-clips-"));
for (const [layout, size] of Object.entries(LAYOUTS)) {
  for (const theme of ["dark", "light"]) {
    const master = join(work, `${layout}-${theme}.mp4`);
    const url = `${base}?reel=${REEL}&theme=${theme}&w=${size.w}&h=${size.h}`;
    const capture = spawnSync("node", [join(repo, ".viris/capture.mjs"), url, master, "30", "1", "4", String(from), String(to)], { stdio: "inherit" });
    if (capture.status !== 0) throw new Error(`capture failed for ${layout}-${theme}`);
    const out = join(outDir, `${REEL}.fallback-${layout}-${theme}.mp4`);
    const encode = spawnSync("ffmpeg", ["-y", "-loglevel", "error", "-i", master, "-vf", `scale=${size.scale}:flags=lanczos,format=yuv420p`,
      "-c:v", "libx264", "-profile:v", "main", "-preset", "slow", "-crf", "30", "-g", "30", "-an", "-movflags", "+faststart", out], { stdio: "inherit" });
    if (encode.status !== 0) throw new Error(`encode failed for ${layout}-${theme}`);
    console.log(`wrote ${out}`);
  }
}
rmSync(work, { recursive: true, force: true });
