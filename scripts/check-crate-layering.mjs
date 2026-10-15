// Caller: scripts/check-crate-layering.sh
import fs from "node:fs";
import path from "node:path";

const root = path.resolve(process.argv[2] ?? path.join(import.meta.dirname, ".."));
const hosts = new Set([
  "codeterm-service-core", "codeterm-api", "codeterm-commands", "codeterm-host-runtime",
  "codeterm-orchestration", "codeterm-sentinel-host", "codeterm-socket",
]);
const leafTiers = [
  [],
  ["events", "config", "secrets", "keepawake", "crash", "cron", "shutdown", "prompts", "local-preview", "worker-services", "lanes"],
  ["sentinel"],
  ["agent-model"],
  ["supporter"],
  ["provider-ops", "recall"],
];
const tierByCrate = new Map(leafTiers.flatMap((names, tier) => names.map((name) => [`codeterm-${name}`, tier])));
// Unlisted foundation crates are tier zero; extracted leaves must never become their dependencies.
const tierFor = (name) => tierByCrate.get(name) ?? 0;

function cargoDependencies(text) {
  const dependencies = [];
  for (const section of text.replace(/#.*$/gm, "").split(/\r?\n(?=\s*\[)/)) {
    const table = /^\s*\[([^\n]+)\]\s*\r?\n([\s\S]*)/.exec(section);
    if (!table) continue;
    const dependencyTable = /(?:^|\.)(?:dependencies|dev-dependencies|build-dependencies)(?:\.["']?([\w-]+)["']?)?$/.exec(table[1]);
    if (!dependencyTable) continue;
    const packageName = (body, fallback) => /\bpackage\s*=\s*["']([^"']+)["']/.exec(body)?.[1] ?? fallback;
    if (dependencyTable[1]) {
      dependencies.push(packageName(table[2], dependencyTable[1]));
    } else {
      for (const entry of table[2].matchAll(/^\s*["']?([\w-]+)["']?\s*=\s*(\{[^}]*\}|[^\n]*)/gm)) {
        dependencies.push(packageName(entry[2], entry[1]));
      }
    }
  }
  return dependencies;
}
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
    if (!entry.isDirectory()) continue;
    const crate = path.join(directory, entry.name);
    const manifest = path.join(crate, "Cargo.toml");
    if (!fs.existsSync(manifest)) continue;
    const dependencies = cargoDependencies(fs.readFileSync(manifest, "utf8"));
    if ((!hosts.has(entry.name) || entry.name === "codeterm-service-core")
        && dependencies.includes("codeterm-host-runtime")) {
      violations.push(`${path.relative(root, manifest).replaceAll("\\", "/")}: core or leaf crate depends on codeterm-host-runtime`);
    }
    if (entry.name === "codeterm-service-core") {
      for (const dependency of dependencies.filter((name) => /^tauri(?:-|$)/.test(name))) {
        violations.push(`${path.relative(root, manifest).replaceAll("\\", "/")}: service core depends on ${dependency}`);
      }
    }
    if (hosts.has(entry.name)) continue;
    for (const dependency of dependencies) {
      if (tierFor(dependency) > tierFor(entry.name)) {
        violations.push(`${path.relative(root, manifest).replaceAll("\\", "/")}: leaf tier inversion: ${entry.name} (tier ${tierFor(entry.name)}) -> ${dependency} (tier ${tierFor(dependency)})`);
      }
    }
    const files = [manifest, path.join(crate, "build.rs"), ...sources(path.join(crate, "src")), ...sources(path.join(crate, "tests"))];
    for (const file of files.filter((file) => fs.existsSync(file))) {
      const isManifest = file === manifest;
      const lines = fs.readFileSync(file, "utf8").split(/\r?\n/);
      lines.forEach((raw, index) => {
        const line = raw.replace(isManifest ? /#.*$/ : /\/\/.*$/, "");
        const core = isManifest ? /\bcodeterm[-_]service[-_]core\b/ : /\bcodeterm_service_core\b/;
        const tauri = isManifest ? /\btauri(?:[-_]\w+)?\b/ : /\btauri(?:_\w+)?\s*::|\b(?:use|extern\s+crate)\s+tauri\b/;
        const location = `${path.relative(root, file).replaceAll("\\", "/")}:${index + 1}`;
        if (!isManifest && /\bServiceContext\b/.test(line)) violations.push(`${location}: leaf crate names ServiceContext`);
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
