import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";

export function cachedPreparation(root, name, inputs, prepare) {
  const receipt = path.join(root, "target", "test-preparation", `${name}.json`);
  const fingerprint = () => createHash("sha256").update(JSON.stringify(inputs.map(file => {
    try {
      const stat = fs.lstatSync(file);
      return [file, stat.mode, stat.ino, stat.size, stat.mtimeMs, stat.ctimeMs];
    } catch (error) { if (error.code === "ENOENT") return [file, "missing"]; throw error; }
  }))).digest("hex");
  const current = fingerprint();
  try { if (fs.readFileSync(receipt, "utf8") === current) return true; } catch {}
  prepare();
  fs.mkdirSync(path.dirname(receipt), { recursive: true });
  fs.writeFileSync(receipt, fingerprint());
  return false;
}
