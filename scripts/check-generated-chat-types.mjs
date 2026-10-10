import { execFileSync } from "node:child_process";
import { readdir } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const directories = [
  "gen",
  "packages/chat-core/src/gen",
  "packages/plugin-sdk/src/gen",
];

async function generatedFiles(directory, relative = "") {
  const files = [];
  for (const entry of await readdir(path.join(root, directory, relative), { withFileTypes: true })) {
    const child = relative ? `${relative}/${entry.name}` : entry.name;
    if (entry.isDirectory()) files.push(...(await generatedFiles(directory, child)));
    else if (entry.isFile() && entry.name.endsWith(".ts")) files.push(child);
  }
  return files;
}

for (const directory of directories) {
  const generated = await generatedFiles(directory);
  if (generated.length === 0) {
    throw new Error(`No generated TypeScript files found under ${directory}`);
  }
  for (const name of generated) {
    const relative = `${directory}/${name}`;
    try {
      execFileSync("git", ["ls-files", "--error-unmatch", "--", relative], {
        cwd: root,
        stdio: "ignore",
      });
    } catch {
      throw new Error(`Generated TypeScript file is not committed: ${relative}`);
    }
  }
}
