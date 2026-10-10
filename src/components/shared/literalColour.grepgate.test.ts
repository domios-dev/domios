import { readdirSync, readFileSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, relative } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, "..", "..", "..");
const ROOTS = ["src/components", "packages/chat-ui/src"];
const SKIP = /\.test\.|testdata|browserHarness|gallery|Gallery/;

// Literal colours that remain on purpose: canvas palettes read tokens at runtime and keep these as fallbacks,
// presence hues are painted on canvas, and the pairing QR code must stay dark on white to scan.
const BASELINE: Record<string, number> = {
  "packages/chat-ui/src": 14,
  "src/components/Canvas": 16,
  "src/components/Extensions": 8,
  "src/components/FrontendCrashBoundary.css": 5,
  "src/components/MemoryPanel": 46,
  "src/components/MeshPanel": 2,
  "src/components/Snake": 101,
  "src/components/Terminal": 25,
  "src/components/UpdateBadge.tsx": 2,
};

const HEX = /(?<![\w&])#(?:[0-9a-fA-F]{8}|[0-9a-fA-F]{6}|[0-9a-fA-F]{3,4})(?![\w-])/g;
const FALLBACKS = [
  /var\(\s*--[\w-]+\s*,\s*#[0-9a-fA-F]{3,8}\s*\)/g,
  /\(\s*\w+\s*,\s*["']--[\w-]+["']\s*,\s*["']#[0-9a-fA-F]{3,8}["']\s*\)/g,
  /\(\s*["']--[\w-]+["']\s*,\s*["']#[0-9a-fA-F]{3,8}["']\s*\)/g,
];

export function literalColours(line: string): number {
  const code = line.trim();
  if (code.startsWith("//") || code.startsWith("*") || code.startsWith("/*")) return 0;
  const withoutComment = code.replace(/\s\/\/.*$/, "").replace(/\/\*.*?\*\//g, "");
  const withoutFallbacks = FALLBACKS.reduce((text, pattern) => text.replace(pattern, ""), withoutComment);
  return (withoutFallbacks.match(HEX) ?? []).filter((hex) => !/^#\d{3,4}$/.test(hex)).length;
}

function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

assert(literalColours(`style={{ color: "#ff5f57" }}`) === 1, "a literal colour counts");
assert(literalColours(`className="bg-[#e81123]"`) === 1, "a Tailwind arbitrary colour counts");
assert(literalColours(`color: var(--ct-fg, #c9d1d9);`) === 0, "a token fallback is allowed");
assert(literalColours(`fg: getCSSVar("--ct-fg", "#c0caf5"),`) === 0, "a canvas token read with a fallback is allowed");
assert(literalColours(`fg: readVar(s, "--ct-fg", "#e6e9ef"),`) === 0, "a palette token read with a fallback is allowed");
assert(literalColours(`// paired peer must never count (codeterm#440).`) === 0, "an issue reference in a comment is not a colour");
assert(literalColours(`const issue = "see #119";`) === 0, "a three-digit issue number is not a colour");

function walk(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    if (entry === "node_modules" || entry === "dist") continue;
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) walk(full, out);
    else if (/\.(ts|tsx|css)$/.test(entry)) out.push(full);
  }
  return out;
}

function surfaceOf(rel: string): string {
  return rel.split("/").slice(0, 3).join("/");
}

const counts: Record<string, number> = {};
const offenders: Record<string, string[]> = {};
for (const root of ROOTS) {
  for (const file of walk(join(repoRoot, root))) {
    const rel = relative(repoRoot, file).split("\\").join("/");
    if (SKIP.test(rel)) continue;
    readFileSync(file, "utf8").split("\n").forEach((line, index) => {
      const hits = literalColours(line);
      if (!hits) return;
      const surface = surfaceOf(rel);
      counts[surface] = (counts[surface] ?? 0) + hits;
      (offenders[surface] ??= []).push(`${rel}:${index + 1}: ${line.trim().slice(0, 120)}`);
    });
  }
}

const failures: string[] = [];
for (const surface of new Set([...Object.keys(counts), ...Object.keys(BASELINE)])) {
  const actual = counts[surface] ?? 0;
  const baseline = BASELINE[surface] ?? 0;
  if (actual > baseline) failures.push(`  ${surface}: ${actual} literal colours (baseline ${baseline}); use --ct-* tokens\n    ${offenders[surface].join("\n    ")}`);
  else if (actual < baseline) failures.push(`  ${surface}: ${actual} literal colours, below baseline ${baseline}; lower the baseline`);
}

assert(failures.length === 0, `Literal colour ratchet:\n${failures.join("\n")}`);
console.log("✓ literal colour ratchet:", Object.entries(counts).map(([k, v]) => `${k}=${v}`).join(", "));
