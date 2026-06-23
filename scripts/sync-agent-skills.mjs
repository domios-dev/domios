import fs from "node:fs";
import path from "node:path";

const root = process.cwd();
const source = path.join(root, ".agents", "skills");
const target = path.join(root, ".claude", "skills");

if (!fs.existsSync(source)) {
  throw new Error(`Missing source skills directory: ${source}`);
}

fs.mkdirSync(target, { recursive: true });

const sourceNames = new Set(
  fs
    .readdirSync(source, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .filter((entry) => fs.existsSync(path.join(source, entry.name, "SKILL.md")))
    .map((entry) => entry.name),
);

for (const name of sourceNames) {
  const from = path.join(source, name);
  const to = path.join(target, name);
  fs.rmSync(to, { recursive: true, force: true });
  fs.cpSync(from, to, { recursive: true });
  console.log(`synced ${name}`);
}

for (const entry of fs.readdirSync(target, { withFileTypes: true })) {
  if (entry.isDirectory() && !sourceNames.has(entry.name)) {
    fs.rmSync(path.join(target, entry.name), { recursive: true, force: true });
    console.log(`removed stale ${entry.name}`);
  }
}
