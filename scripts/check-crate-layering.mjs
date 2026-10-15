// Caller: scripts/check-crate-layering.sh
import fs from "node:fs";
import path from "node:path";

const root = path.resolve(process.argv[2] ?? path.join(import.meta.dirname, ".."));
const hosts = new Set([
  "codeterm-service-core", "codeterm-api", "codeterm-commands",
  "codeterm-orchestration", "codeterm-sentinel-host", "codeterm-socket",
]);
const knownTauriViolations = new Map(
  (process.env.CODETERM_LAYERING_KNOWN_TAURI ?? "").split(/\s+/).filter(Boolean).map((entry) => entry.split(":")),
);

function sources(directory) {
  if (!fs.existsSync(directory)) return [];
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const file = path.join(directory, entry.name);
    if (entry.isDirectory()) return sources(file);
    return entry.isFile() && file.endsWith(".rs") ? [file] : [];
  });
}

const violations = [];
const seenTauri = new Set();
for (const base of ["src-tauri/crates", "crates"]) {
  const directory = path.join(root, base);
  if (!fs.existsSync(directory)) continue;
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    if (!entry.isDirectory() || hosts.has(entry.name)) continue;
    const crate = path.join(directory, entry.name);
    const manifest = path.join(crate, "Cargo.toml");
    if (!fs.existsSync(manifest)) continue;
    const files = [manifest, path.join(crate, "build.rs"), ...sources(path.join(crate, "src")), ...sources(path.join(crate, "tests"))];
    for (const file of files.filter((file) => fs.existsSync(file))) {
      const isManifest = file === manifest;
      const lines = fs.readFileSync(file, "utf8").split(/\r?\n/);
      lines.forEach((raw, index) => {
        const line = raw.replace(isManifest ? /#.*$/ : /\/\/.*$/, "");
        const core = isManifest ? /\bcodeterm[-_]service[-_]core\b/ : /\bcodeterm_service_core\b/;
        const tauri = isManifest ? /\btauri(?:[-_]\w+)?\b/ : /\btauri(?:_\w+)?\s*::|\b(?:use|extern\s+crate)\s+tauri\b/;
        const location = `${path.relative(root, file).replaceAll("\\", "/")}:${index + 1}`;
        if (core.test(line)) violations.push(`${location}: leaf crate names codeterm-service-core`);
        if (tauri.test(line)) {
          seenTauri.add(entry.name);
          if (!knownTauriViolations.has(entry.name)) violations.push(`${location}: leaf crate names tauri`);
        }
      });
    }
  }
}
for (const [crate, task] of knownTauriViolations) {
  if (!seenTauri.has(crate)) violations.push(`${crate}: stale known tauri violation; remove entry (${task})`);
}
if (violations.length) {
  console.error(violations.join("\n"));
  process.exitCode = 1;
} else {
  console.log("crate layering: ok");
}
