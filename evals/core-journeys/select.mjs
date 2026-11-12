import { execFileSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { loadAreas, selectorOutput, staleAreas } from "./lib/selection.mjs";

const USAGE = `Usage: node evals/core-journeys/select.mjs BASE HEAD [--out FILE]
Selects the core journeys whose areas (evals/core-journeys/areas.json) the diff BASE..HEAD touches.
Prints the selection JSON, or writes it to FILE for run.sh --selection FILE. Unmapped paths select nothing.
Exit 0: selection written; 2: usage, git or stale area map error.`;

const git = (...args) => execFileSync("git", args, { encoding: "utf8", maxBuffer: 1 << 28 }).split("\n").map((line) => line.trim()).filter(Boolean);

export function main(argv) {
  const [base, head, flag, out] = argv;
  if (!base || !head || base.startsWith("--") || head.startsWith("--") || (flag !== undefined && (flag !== "--out" || !out)) || argv.length > 4) {
    console.error(USAGE);
    return 2;
  }
  const areas = loadAreas();
  const stale = staleAreas(areas, git("ls-tree", "-r", "--name-only", head));
  if (stale.length) {
    console.error(`FAIL areas.json names prefixes that match no file at ${head}; update the map:\n  ${stale.join("\n  ")}`);
    return 2;
  }
  const [baseSha] = git("rev-parse", "--verify", `${base}^{commit}`), [headSha] = git("rev-parse", "--verify", `${head}^{commit}`);
  const paths = git("diff", "--name-only", "--no-renames", `${baseSha}..${headSha}`);
  const text = `${JSON.stringify(selectorOutput(areas, { base: baseSha, head: headSha, paths }), null, 2)}\n`;
  if (out) writeFileSync(resolve(out), text);
  else process.stdout.write(text);
  return 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { process.exitCode = main(process.argv.slice(2)); } catch (error) {
    console.error(`FAIL ${error.message}`);
    process.exitCode = 2;
  }
}
