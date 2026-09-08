import fs from "node:fs";
import path from "node:path";
import { randomUUID } from "node:crypto";
import { pathToFileURL } from "node:url";

function normalized(value) {
  const result = path.resolve(value).replaceAll("\\", "/");
  return process.platform === "win32" ? result.toLowerCase() : result;
}

export function prepareLocalInstallCache(target) {
  const absolute = path.resolve(target);
  const parent = path.dirname(absolute);
  if (path.basename(parent) !== "local-install" || !/^[a-f0-9]{40}$/.test(path.basename(absolute))) {
    throw new Error("Expected one canonical local-install cache identity");
  }
  if (!fs.existsSync(absolute)) {
    fs.mkdirSync(absolute, { recursive: true });
    return null;
  }
  if (normalized(fs.realpathSync(absolute)) !== normalized(absolute)) {
    throw new Error("Refusing a redirected local-install cache");
  }
  const build = path.join(absolute, "release", "build");
  if (!fs.existsSync(build)) return null;
  const stale = [];
  for (const entry of fs.readdirSync(build, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const owner = path.join(build, entry.name);
    const marker = path.join(owner, "root-output");
    if (!fs.existsSync(marker)) continue;
    const recorded = fs.readFileSync(marker, "utf8").trim();
    if (normalized(recorded) !== normalized(path.join(owner, "out"))) stale.push(entry.name);
  }
  if (stale.length === 0) return null;
  const backup = path.join(parent, `${path.basename(absolute)}.relocated-${randomUUID()}`);
  if (path.dirname(backup) !== parent || fs.existsSync(backup)) throw new Error("Unsafe cache backup destination");
  fs.renameSync(absolute, backup);
  fs.mkdirSync(absolute);
  return { backup, stale };
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const target = process.argv[2];
  if (!target) throw new Error("Missing local-install target");
  const result = prepareLocalInstallCache(target);
  if (result) console.log(`[cache-policy] preserved relocated build cache at ${result.backup}; regenerating ${result.stale.length} stale build-script owners`);
}
