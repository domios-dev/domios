#!/usr/bin/env node
// WHAT: manual-tier Account Mesh rollout harness over real installations A/B (paired), an Add fixture and a third machine C.
// WHY: implementation lanes cannot prove upgrade continuity, sign-out blocking, server slot refusal or isolation from source.
// ORACLE: real peer pair records, mesh put/get round trips, server machine counts and Browser text read from the targets themselves.
// Usage: node scripts/account-mesh-e2e.mjs --manifest ABS.json --out ABS_DIR [--preflight-only] [--phase ID]
// Exit: 0 pass/preflight pass, 1 runtime fail, 2 refused before effects, 4 held on missing receipts.
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, isAbsolute } from "node:path";
import { fileURLToPath } from "node:url";
import { DeadlineError, createDriver, preflight } from "./account-mesh-e2e/driver.mjs";
import { CLASSIFICATION, classify, openJournal } from "./account-mesh-e2e/journal.mjs";
import { validateManifest } from "./account-mesh-e2e/manifest.mjs";
import { PHASES, phaseReadiness } from "./account-mesh-e2e/phases.mjs";
import { runBounded } from "./account-mesh-e2e/runner.mjs";

const EXIT = { [CLASSIFICATION.PASS]: 0, [CLASSIFICATION.PREFLIGHT_PASS]: 0, [CLASSIFICATION.FAIL]: 1, [CLASSIFICATION.REFUSED]: 2, [CLASSIFICATION.HELD]: 4 };

export function parseArgs(argv) {
  const args = { manifest: null, out: null, preflightOnly: false, phase: null, errors: [] };
  for (let i = 0; i < argv.length; i += 1) {
    const a = argv[i];
    if (a === "--manifest" || a === "--out" || a === "--phase") {
      const value = argv[i + 1];
      if (!value || value.startsWith("--")) args.errors.push(`${a} needs a value`);
      else args[a === "--manifest" ? "manifest" : a === "--out" ? "out" : "phase"] = value;
      i += 1;
    } else if (a === "--preflight-only") args.preflightOnly = true;
    else args.errors.push(`unknown argument ${a}`);
  }
  if (!args.manifest || !isAbsolute(args.manifest)) args.errors.push("--manifest ABSOLUTE_PATH is required");
  if (!args.out || !isAbsolute(args.out)) args.errors.push("--out ABSOLUTE_DIR is required");
  if (args.phase && !PHASES.some((p) => p.id === args.phase)) args.errors.push(`--phase must be one of ${PHASES.map((p) => p.id).join(", ")}`);
  if (args.phase && args.preflightOnly) args.errors.push("--phase and --preflight-only are exclusive");
  return args;
}

function fileDigest(path) {
  try { return createHash("sha256").update(readFileSync(path)).digest("hex"); } catch { return null; }
}

async function harnessSha(run) {
  const here = dirname(fileURLToPath(import.meta.url));
  const r = await run(["git", "-C", here, "rev-parse", "HEAD"], { deadlineMs: 10_000 });
  return r.exit === 0 ? r.stdout.trim() : null;
}

export async function main(argv, { run = runBounded, log = (s) => process.stdout.write(`${s}\n`), env = process.env } = {}) {
  const args = parseArgs(argv);
  if (args.errors.length) {
    for (const e of args.errors) log(`refused: ${e}`);
    return EXIT[CLASSIFICATION.REFUSED];
  }
  let manifest;
  try {
    manifest = JSON.parse(readFileSync(args.manifest, "utf8"));
  } catch (error) {
    log(`refused: manifest unreadable: ${error.message}`);
    return EXIT[CLASSIFICATION.REFUSED];
  }
  const validation = validateManifest(manifest, { outDir: args.out });
  if (!validation.ok) {
    for (const e of validation.errors) log(`refused: ${e}`);
    return EXIT[CLASSIFICATION.REFUSED];
  }
  let journal;
  try {
    journal = openJournal(args.out, validation.digest, { phase: args.phase });
  } catch (error) {
    log(`refused: ${error.message}`);
    return EXIT[CLASSIFICATION.REFUSED];
  }
  const results = { manifest_sha256: validation.digest, harness_sha: await harnessSha(run), run_id: manifest.run_id, release: manifest.release, refused: [], preflight: [], phases: [] };
  journal.event("start", { preflight_only: args.preflightOnly, phase: args.phase, harness_sha: results.harness_sha });
  const driver = createDriver({ manifest, journal, run, env });

  try {
    results.preflight = await preflight(driver);
  } catch (error) {
    results.refused.push(`preflight aborted: ${error.message}`);
  }
  const preflightOk = !results.refused.length && results.preflight.every((c) => c.status === "PASS");
  if (preflightOk && !args.preflightOnly) {
    for (const phase of PHASES.filter((p) => !args.phase || p.id === args.phase)) {
      const missing = phaseReadiness(manifest, phase, { receiptDigest: fileDigest });
      const entry = { id: phase.id, cleanup_owner: phase.cleanup_owner, missing, checks: [], status: missing.length ? "HELD" : "RUN" };
      results.phases.push(entry);
      if (missing.length) {
        journal.event("phase-held", { phase: phase.id, missing });
        continue;
      }
      journal.event("phase-start", { phase: phase.id });
      driver.setPhaseDeadline(manifest.deadlines.phase_ms);
      try {
        await phase.run(driver, entry.checks);
      } catch (error) {
        entry.checks.push({ id: `${phase.id}-aborted`, status: "FAIL", reasons: [`${error instanceof DeadlineError ? "deadline" : "error"}: ${error.message}`], evidence: {} });
      } finally {
        if (phase.cleanup) {
          driver.clearPhaseDeadline();
          try { await phase.cleanup(driver); } catch (error) { entry.checks.push({ id: `${phase.id}-cleanup`, status: "FAIL", reasons: [`cleanup failed, owner: ${phase.cleanup_owner}: ${error.message}`], evidence: {} }); }
        }
        driver.clearPhaseDeadline();
        journal.event("phase-end", { phase: phase.id, failed: entry.checks.filter((c) => c.status !== "PASS").map((c) => c.id) });
      }
    }
  }
  results.classification = classify({ refused: results.refused, preflight: results.preflight, phases: results.phases, preflightOnly: args.preflightOnly || Boolean(!preflightOk) });
  if (args.phase && results.classification === CLASSIFICATION.PASS) results.classification = CLASSIFICATION.HELD;
  const name = journal.writeResults(results);
  journal.event("end", { classification: results.classification, results: name });
  log(`${results.classification}: ${args.out}/${name}.md`);
  return EXIT[results.classification];
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  main(process.argv.slice(2)).then((code) => { process.exitCode = code; });
}
