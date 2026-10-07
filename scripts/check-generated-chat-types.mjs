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

for (const directory of directories) {
  const generated = (await readdir(path.join(root, directory))).filter((name) =>
    name.endsWith(".ts"),
  );
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
