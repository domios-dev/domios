import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { digest, logLines, newEvidence } from "./logs.mjs";

const options = new Map();
const args = process.argv.slice(2);
for (let i = 0; i < args.length; i += 2) {
  if (!["--out", "--from-results", "--log-dir"].includes(args[i]) || !args[i + 1]) {
    throw new Error("Usage: --out DIR --from-results FILE --log-dir DIR");
  }
  options.set(args[i], args[i + 1]);
}
for (const name of ["--out", "--from-results", "--log-dir"]) {
  if (!options.has(name)) throw new Error(`Missing ${name}`);
}
const source = resolve(options.get("--from-results"));
const out = resolve(options.get("--out"));
const target = join(out, "dry-run.json");
if (existsSync(join(out, "results.json")) || target === source) {
  throw new Error("Dry run requires a separate output directory; live evidence is preserved");
}
const result = JSON.parse(readFileSync(source, "utf8"));
const lines = logLines(resolve(options.get("--log-dir")));
const baseline = lines.map(digest);
const evidence = newEvidence(lines, result.log_baseline ?? []);
mkdirSync(out, { recursive: true });
writeFileSync(target, `${JSON.stringify({
  dry_run: true, source, log_lines: lines.length, baseline_hashes: baseline.length,
  captured_evidence_lines: evidence.length, fixture_count: result.fixtures?.length ?? 0,
  round_zero_failures: result.rounds?.find((entry) => entry.round === 0)?.rows.filter((row) => !row.pass).map((row) => ({ id: row.id, reason: row.reason })) ?? [],
  planned_rounds: [1, 2, 3], cli_calls: 0, api_calls: 0, notice_hooks: 0, dev_restarts: 0,
}, null, 2)}\n`);
writeFileSync(join(out, "dry-run-evidence.log"), `${evidence.join("\n")}\n`);
console.log(`DRY RUN PASS: captured ${lines.length} log lines (${baseline.length} hashes); no CLI, API, notice hook or DEV restart. Receipt: ${target}`);
