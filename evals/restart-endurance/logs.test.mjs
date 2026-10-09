import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { digest, logLines, newEvidence } from "./logs.mjs";

test("large restart log capture exceeds the former argument limit without overflowing", () => {
  const directory = mkdtempSync(join(tmpdir(), "endure-logs-"));
  try {
    const count = 250_000;
    writeFileSync(join(directory, "codeterm.log"), Array.from({ length: count }, (_, i) => `${i} [restore] tab=sample`).join("\n"));
    writeFileSync(join(directory, "codeterm.log.1"), "old [agent-launch] tab=sample\n");
    writeFileSync(join(directory, "unrelated.log"), "ignored\n");
    const lines = logLines(directory);
    assert.equal(lines.length, count + 1);
    assert.equal(newEvidence(lines, lines.map(digest)).length, 0);
    assert.deepEqual(newEvidence(["old [restore]", "new launch failed", "ordinary"], [digest("old [restore]")]), ["new launch failed"]);

    const source = join(directory, "manifest.json");
    const original = JSON.stringify({ fixtures: [{ id: "sample" }], rounds: [{ round: 0, rows: [{ id: "sample", pass: false, reason: "preserved" }] }] });
    writeFileSync(source, original);
    const out = join(directory, "receipt");
    const execution = spawnSync(process.execPath, [fileURLToPath(new URL("./dry-run.mjs", import.meta.url)),
      "--out", out, "--from-results", source, "--log-dir", directory], {
      encoding: "utf8", env: { ...process.env, CODETERM_API_PORT: "7685", CODETERM_API_URL: "http://127.0.0.1:7685" }, timeout: 30_000,
    });
    assert.equal(execution.status, 0, execution.stderr);
    const receipt = JSON.parse(readFileSync(join(out, "dry-run.json"), "utf8"));
    assert.equal(receipt.log_lines, count + 1);
    for (const name of ["cli_calls", "api_calls", "notice_hooks", "dev_restarts"]) assert.equal(receipt[name], 0);
    assert.equal(receipt.round_zero_failures[0].reason, "preserved");
    assert.equal(readFileSync(source, "utf8"), original);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});
