import { appendFileSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { isAbsolute, join } from "node:path";
import { digest } from "./manifest.mjs";

const SECRET_KEY = /(token|secret|password|credential|private|cookie|authorization|id_token)/i;
const ADDRESS_KEY = /^(addresses|local_ip|tailscale_ip|ip|host|endpoint_address|lan_addresses)$/i;
const IDENTITY_KEY = /^(uid|email|display_name|account_id|user_id)$/i;

export function redact(value) {
  if (Array.isArray(value)) return value.map(redact);
  if (!value || typeof value !== "object") return value;
  const out = {};
  for (const [key, v] of Object.entries(value)) {
    if (key === "public_key" && typeof v === "string") out.public_key_sha256 = digest(v);
    else if (SECRET_KEY.test(key)) out[key] = "[redacted]";
    else if (ADDRESS_KEY.test(key)) out[key] = v === null ? null : "[address redacted]";
    else if (IDENTITY_KEY.test(key)) out[`${key}_sha256`] = v === null ? null : digest(String(v));
    else out[key] = redact(v);
  }
  return out;
}

export function commandEvidence(result) {
  return {
    argv: result.argv,
    exit: result.exit,
    timed_out: result.timedOut,
    spawn_error: result.spawnError === true,
    ms: result.ms,
    stdout_sha256: digest(result.stdout ?? ""),
    stdout_bytes: Buffer.byteLength(result.stdout ?? ""),
    stderr_excerpt: String(result.stderr ?? "").replace(/\b\d{1,3}(\.\d{1,3}){3}\b/g, "[ip]").slice(0, 600),
    json: redact(result.json ?? null),
  };
}

// An evidence directory belongs to one manifest: a new run needs an empty directory, and a
// single-phase repeat must present the same manifest digest that created it.
export function openJournal(outDir, manifestDigest, { phase = null } = {}) {
  if (!isAbsolute(outDir)) throw new Error(`--out must be absolute: ${outDir}`);
  mkdirSync(outDir, { recursive: true });
  const runPath = join(outDir, "run.json");
  if (existsSync(runPath)) {
    const run = JSON.parse(readFileSync(runPath, "utf8"));
    if (run.manifest_sha256 !== manifestDigest) throw new Error(`${outDir} belongs to manifest ${run.manifest_sha256}, not ${manifestDigest}`);
    if (!phase) throw new Error(`${outDir} already holds a run; repeat one case with --phase or use a new directory`);
  } else {
    const entries = readdirSync(outDir);
    if (entries.length) throw new Error(`${outDir} is not empty and holds no run.json; refusing to mix evidence`);
    writeFileSync(runPath, `${JSON.stringify({ manifest_sha256: manifestDigest, created_at: new Date().toISOString() }, null, 2)}\n`, { flag: "wx" });
  }
  mkdirSync(join(outDir, "artifacts"), { recursive: true });
  let seq = readdirSync(join(outDir, "artifacts")).length;
  const eventsPath = join(outDir, "events.jsonl");
  return {
    dir: outDir,
    event(kind, data = {}) {
      appendFileSync(eventsPath, `${JSON.stringify({ at: new Date().toISOString(), kind, ...data })}\n`);
    },
    artifact(name, data) {
      seq += 1;
      const file = join("artifacts", `${String(seq).padStart(4, "0")}-${name.replace(/[^a-z0-9.-]/gi, "_")}.json`);
      writeFileSync(join(outDir, file), `${JSON.stringify(data, null, 2)}\n`, { flag: "wx" });
      return file;
    },
    writeResults(results) {
      const stamp = String(Date.now());
      writeFileSync(join(outDir, `results-${stamp}.json`), `${JSON.stringify(results, null, 2)}\n`, { flag: "wx" });
      writeFileSync(join(outDir, `results-${stamp}.md`), renderMarkdown(results), { flag: "wx" });
      return `results-${stamp}`;
    },
  };
}

export const CLASSIFICATION = Object.freeze({
  REFUSED: "refused-before-effects",
  PREFLIGHT_PASS: "preflight-pass-runtime-not-run",
  HELD: "source-ready-runtime-held",
  FAIL: "runtime-fail",
  PASS: "real-machine-pass-awaiting-turner-acceptance",
});

export function classify({ refused = [], preflight = [], phases = [], preflightOnly = false }) {
  if (refused.length) return CLASSIFICATION.REFUSED;
  if (preflight.some((c) => c.status !== "PASS")) return CLASSIFICATION.REFUSED;
  if (preflightOnly) return CLASSIFICATION.PREFLIGHT_PASS;
  const checks = phases.flatMap((p) => p.checks ?? []);
  if (checks.some((c) => c.status === "FAIL")) return CLASSIFICATION.FAIL;
  if (phases.length === 0 || phases.some((p) => p.status !== "RUN") || checks.some((c) => c.status !== "PASS")) return CLASSIFICATION.HELD;
  return CLASSIFICATION.PASS;
}

function renderMarkdown(results) {
  const lines = [`# Account Mesh real-machine evidence`, "", `Classification: **${results.classification}**`, ""];
  lines.push(`Manifest sha256: \`${results.manifest_sha256 ?? "invalid"}\``, `Harness sha: \`${results.harness_sha ?? "unknown"}\``, "");
  if (results.refused?.length) lines.push("## Refused before effects", "", ...results.refused.map((r) => `- ${r}`), "");
  if (results.preflight?.length) {
    lines.push("## Preflight", "", "| Check | Status | Reasons |", "|---|---|---|");
    for (const c of results.preflight) lines.push(`| ${c.id} | ${c.status} | ${c.reasons.join("; ")} |`);
    lines.push("");
  }
  for (const phase of results.phases ?? []) {
    lines.push(`## ${phase.id} — ${phase.status}`, "", `Cleanup owner: ${phase.cleanup_owner}`, "");
    if (phase.missing?.length) lines.push(...phase.missing.map((m) => `- missing: ${m}`), "");
    if (phase.checks?.length) {
      lines.push("| Check | Status | Reasons |", "|---|---|---|");
      for (const c of phase.checks) lines.push(`| ${c.id} | ${c.status} | ${c.reasons.join("; ")} |`);
      lines.push("");
    }
  }
  lines.push("A harness or source pass is not a release receipt; Turner accepts real-machine evidence and runs the release gate.", "");
  return lines.join("\n");
}
