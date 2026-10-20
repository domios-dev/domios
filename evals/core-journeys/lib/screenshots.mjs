import { execFile } from "node:child_process";
import { mkdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { promisify } from "node:util";
import { root } from "./env.mjs";

const run = promisify(execFile);
const SCRIPT = join(root, "scripts/dev-screenshot.mjs");

export const screenshotPath = (phase) => `screenshots/${phase}.png`;

export function screenshotCommand(out, phase, port) {
  return [process.execPath, [SCRIPT, join(out, screenshotPath(phase)), "--api-port", String(port)]];
}

export function captureScreenshot(ctx, phase) {
  return ctx.check("EVIDENCE", "window_screenshot", phase, async () => {
    mkdirSync(join(ctx.out, "screenshots"), { recursive: true });
    const [command, args] = screenshotCommand(ctx.out, phase, ctx.dev.port);
    await run(command, args, { cwd: root, timeout: 60_000 });
    const file = screenshotPath(phase);
    const bytes = statSync(join(ctx.out, file)).size;
    ctx.state.screenshots = [...(ctx.state.screenshots ?? []).filter((shot) => shot.phase !== phase), { phase, file, bytes }];
    ctx.save();
    return { summary: `${file} ${bytes} bytes`, evidence: { file, bytes } };
  });
}
