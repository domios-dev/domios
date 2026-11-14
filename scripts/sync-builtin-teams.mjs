import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const [commit, source = "https://github.com/domios-dev/domios-teams.git"] = process.argv.slice(2);
if (!/^[0-9a-f]{40}$/.test(commit ?? "")) {
  throw new Error("Usage: node scripts/sync-builtin-teams.mjs FULL_COMMIT_SHA [REPOSITORY_URL_OR_PATH]");
}
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const destination = path.join(root, "src-tauri", "team-templates");
const temporary = fs.mkdtempSync(path.join(os.tmpdir(), "domios-team-sync-"));
const staging = fs.mkdtempSync(path.join(path.dirname(destination), ".team-templates-"));
const backup = `${staging}.previous`;
const run = args => execFileSync("git", args, { cwd: temporary, timeout: 120000 });
try {
  run(["init", "--quiet"]);
  run(["fetch", "--no-tags", "--depth=1", "--", source, commit]);
  if (run(["rev-parse", "FETCH_HEAD"]).toString().trim() !== commit) throw new Error("Source commit mismatch");
  const index = JSON.parse(run(["show", `${commit}:channel.json`]));
  if (index.schemaVersion !== 1 || index.name !== "domios-teams" || !Array.isArray(index.teams) || !index.teams.length) {
    throw new Error("Invalid official team index");
  }
  const folders = index.teams.map(team => team.path);
  if (folders.some(folder => typeof folder !== "string" || !/^[a-z0-9][a-z0-9-]*$/.test(folder)) ||
      new Set(folders).size !== folders.length) throw new Error("Unsafe or duplicate package path");
  const selected = ["LICENSE", "README.md", ...folders];
  const entries = run(["ls-tree", "-rz", "--full-tree", commit, "--", ...selected]).toString().split("\0").filter(Boolean);
  for (const entry of entries) {
    const match = /^(100644|100755) blob ([0-9a-f]{40})\t(.+)$/.exec(entry);
    if (!match) throw new Error(`Unsupported snapshot entry: ${entry}`);
    const relative = match[3];
    if (relative.split("/").some(part => !part || part === "." || part === ".." || part.includes("\\") ||
        /[<>:"|?*]/.test(part) || /[. ]$/.test(part) || /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(part))) {
      throw new Error(`Unsafe snapshot path: ${relative}`);
    }
    const target = path.join(staging, ...relative.split("/"));
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, run(["cat-file", "blob", match[2]]));
  }
  for (const file of ["LICENSE", "README.md", ...folders.map(folder => `${folder}/team.yaml`)]) {
    if (!fs.existsSync(path.join(staging, file))) throw new Error(`Incomplete snapshot: ${file}`);
  }
  fs.writeFileSync(path.join(staging, "SOURCE_COMMIT"), `${commit}\n`);
  fs.renameSync(destination, backup);
  try { fs.renameSync(staging, destination); }
  catch (error) { fs.renameSync(backup, destination); throw error; }
  fs.rmSync(backup, { recursive: true, force: true });
  console.log(`Bundled teams refreshed from domios-dev/domios-teams@${commit}`);
} finally {
  fs.rmSync(temporary, { recursive: true, force: true });
  fs.rmSync(staging, { recursive: true, force: true });
}
