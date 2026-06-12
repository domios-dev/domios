import { readdirSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";

const suitesDir = fileURLToPath(new URL("../suites", import.meta.url));
const tagRe = /@(smoke|golden|quarantine)/;
const testTitleRe = /\btest\s*\(\s*(["'`])((?:\\.|(?!\1)[\s\S])*?)\1/g;
const untagged = [];

function specs(dir) {
  const out = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      out.push(...specs(path));
    } else if (entry.isFile() && entry.name.endsWith(".spec.ts")) {
      out.push(path);
    }
  }
  return out;
}

for (const file of specs(suitesDir)) {
  const text = readFileSync(file, "utf8");
  for (const match of text.matchAll(testTitleRe)) {
    const title = match[2].replace(/\\(["'`])/g, "$1");
    if (!tagRe.test(title)) {
      untagged.push(`${file}: ${title}`);
    }
  }
}

if (untagged.length) {
  console.error("Untagged Playwright tests:");
  console.error(untagged.join("\n"));
  process.exit(1);
}
