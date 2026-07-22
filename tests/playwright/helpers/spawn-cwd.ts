import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

export function tempSpawnCwd(prefix = "pw-agent-"): string {
  return mkdtempSync(join(tmpdir(), prefix));
}
