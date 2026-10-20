import { readdirSync, readFileSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, relative } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, "..", "..", "..");
const ROOTS = ["src", "packages/chat-ui/src"];
const SKIP = /\.test\.|testdata|browserHarness|gallery|Gallery/;

// Edge-anchored chrome that is not a floating popover: each stays inside its own box or is clamped by its owner.
const ALLOWED: Record<string, { count: number; reason: string }> = {
  "src/components/AgentPicker/AgentProviderPicker.tsx": { count: 1, reason: "static caption under a provider card" },
  "src/components/Canvas/EssenceMapProjectView.tsx": { count: 1, reason: "canvas node label" },
  "src/components/Notifications/NotificationToast.tsx": { count: 1, reason: "toast stack whose width useToastHostWidth clamps" },
  "src/components/Terminal/ComposerActionButton.tsx": { count: 1, reason: "voice-lock gesture pill above the mic" },
  "src/components/Terminal/ComposerCapsuleField.tsx": { count: 1, reason: "focus-revealed inline field" },
  "src/components/Surface/surface.css": { count: 1, reason: "snip size label inside the snip box" },
  "src/components/Terminal/composerContext.css": { count: 1, reason: "context pill riding on the composer" },
  "src/components/Terminal/tabTopBar.css": { count: 1, reason: "scroll shadow under a header" },
  "src/components/Usage/styles.css": { count: 1, reason: "tip arrow drawn on the tip's own edge" },
};

const TAILWIND = /\babsolute\b.*\b(?:top|bottom)-full\b|\b(?:top|bottom)-full\b.*\babsolute\b/;
const CSS = /^\s*(?:top|bottom)\s*:\s*(?:calc\(\s*)?100%/;

export function handPositioned(line: string, css: boolean): boolean {
  const code = line.trim();
  if (code.startsWith("//") || code.startsWith("*") || code.startsWith("/*")) return false;
  return css ? CSS.test(line) : TAILWIND.test(line);
}

function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

assert(handPositioned(`className="ct-menu absolute bottom-full left-0 mb-1"`, false), "a Tailwind popover above its trigger is refused");
assert(handPositioned(`className="top-full absolute right-0 z-10"`, false), "class order does not matter");
assert(handPositioned(`  top: calc(100% + 4px);`, true), "a CSS popover under its trigger is refused");
assert(handPositioned(`  bottom: 100%;`, true), "a CSS popover above its trigger is refused");
assert(!handPositioned(`className="absolute inset-0"`, false), "ordinary absolute layout is allowed");
assert(!handPositioned(`  max-height: calc(100% - 8px);`, true), "a size is not a position");
assert(!handPositioned(`// absolute top-full used to be here`, false), "comments are ignored");

function walk(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    if (entry === "node_modules" || entry === "dist") continue;
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) walk(full, out);
    else if (/\.(ts|tsx|css)$/.test(entry)) out.push(full);
  }
  return out;
}

const hits: Record<string, string[]> = {};
for (const root of ROOTS) {
  for (const file of walk(join(repoRoot, root))) {
    const rel = relative(repoRoot, file).split("\\").join("/");
    if (SKIP.test(rel)) continue;
    const css = rel.endsWith(".css");
    readFileSync(file, "utf8").split("\n").forEach((line, index) => {
      if (handPositioned(line, css)) (hits[rel] ??= []).push(`${rel}:${index + 1}: ${line.trim().slice(0, 140)}`);
    });
  }
}

const failures: string[] = [];
for (const file of new Set([...Object.keys(hits), ...Object.keys(ALLOWED)])) {
  const found = hits[file]?.length ?? 0;
  const allowed = ALLOWED[file]?.count ?? 0;
  if (found > allowed) failures.push(`  ${file}: ${found} hand-positioned popovers (allowed ${allowed})\n    ${hits[file].join("\n    ")}`);
  else if (found < allowed) failures.push(`  ${file}: ${found} edge anchors, below the allowance ${allowed}; lower it`);
}

assert(
  failures.length === 0,
  `Popovers float through AnchoredPopover (src/components/shared), which flips and clamps inside the visual viewport:\n${failures.join("\n")}`,
);
console.log(`✓ anchored popover gate: ${Object.values(hits).flat().length} allowed edge anchors`);
