// One-shot codemod: point all Tauri core/event imports in src/ at the
// web bridge (src/lib/bridge.ts). Idempotent; safe to re-run.
import { readFileSync, writeFileSync } from "fs";
import { execSync } from "child_process";
import path from "path";

const files = execSync(
  'git grep -l "@tauri-apps/api/core\\|@tauri-apps/api/event" -- "src/**/*.ts" "src/**/*.tsx"',
  { encoding: "utf8" },
)
  .trim()
  .split("\n")
  .filter(Boolean);

let changed = 0;
for (const f of files) {
  const norm = f.replace(/\\/g, "/");
  if (norm === "src/lib/bridge.ts") continue;
  const dir = path.dirname(f);
  let rel = path.relative(dir, "src/lib/bridge").replace(/\\/g, "/");
  if (!rel.startsWith(".")) rel = "./" + rel;
  let src = readFileSync(f, "utf8");
  const before = src;
  src = src.replace(/from "@tauri-apps\/api\/core"/g, `from "${rel}"`);
  src = src.replace(/from "@tauri-apps\/api\/event"/g, `from "${rel}"`);
  if (src !== before) {
    writeFileSync(f, src);
    changed++;
  }
}
console.log(`rewrote ${changed} files`);
