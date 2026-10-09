import { createHash } from "node:crypto";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

export function logLines(directory) {
  const lines = [];
  for (const name of readdirSync(directory).filter((name) => /^codeterm.*\.log(?:\.\d+)?$/.test(name)).sort()) {
    for (const line of readFileSync(join(directory, name), "utf8").split("\n")) {
      if (line) lines.push(line);
    }
  }
  return lines;
}
export const digest = (line) => createHash("sha256").update(line).digest("hex");
export function newEvidence(lines, baseline) {
  const previous = new Set(baseline);
  return lines.filter((line) => !previous.has(digest(line)) &&
    /\[restore\]|\[agent-launch\]|cleared.*identity|launch failed/i.test(line));
}
