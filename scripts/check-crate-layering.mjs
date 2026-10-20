// Caller: scripts/check-crate-layering.sh
import fs from "node:fs";
import path from "node:path";

const root = path.resolve(process.argv[2] ?? path.join(import.meta.dirname, ".."));
const legacyPrefix = "codeterm"; // rename-bridge
const hosts = new Set([
  "domios-server",
  "domios-service-core", "domios-api", "domios-commands",
  "domios-orchestration", "domios-sentinel-host", "domios-socket",
  `${legacyPrefix}-service-core`, `${legacyPrefix}-api`, `${legacyPrefix}-commands`,
  `${legacyPrefix}-orchestration`, `${legacyPrefix}-sentinel-host`, `${legacyPrefix}-socket`,
]);
// Process layers: the native window links only client and shared crates, shared crates never reach
// the server layer, and no other crate depends on the client. Every unlisted crate is server layer.
const layers = JSON.parse(fs.readFileSync(path.join(import.meta.dirname, "crate-layers.json"), "utf8"));
const clientCrates = new Set(layers.client);
const sharedCrates = new Set(layers.shared);
const serverOnly = layers.serverOnly;
const canonicalName = (name) => name === legacyPrefix ? "domios" : name.startsWith(`${legacyPrefix}-`) ? `domios-${name.slice(legacyPrefix.length + 1)}` : name; // rename-bridge
const isClient = (name) => clientCrates.has(canonicalName(name));
const layerOf = (name) => (isClient(name) ? "client" : sharedCrates.has(canonicalName(name)) ? "shared" : "server");
const leafTiers = [
  [],
  ["events", "config", "secrets", "keepawake", "crash", "cron", "shutdown", "prompts", "local-preview", "worker-services", "lanes"],
  ["sentinel"],
  ["agent-model"],
  ["supporter"],
  ["provider-ops", "recall"],
  ["surface", "notifications"],
];
const tierByCrate = new Map(leafTiers.flatMap((names, tier) => names.map((name) => [`domios-${name}`, tier])));
const tierFor = (name) => tierByCrate.get(name.startsWith(`${legacyPrefix}-`) ? `domios-${name.slice(legacyPrefix.length + 1)}` : name) ?? 0; // rename-bridge
// Unlisted foundation crates are tier zero; extracted leaves must never become their dependencies.

function cargoEdges(text) {
  const edges = [];
  for (const section of text.replace(/#.*$/gm, "").split(/\r?\n(?=\s*\[)/)) {
    const table = /^\s*\[([^\n]+)\]\s*\r?\n([\s\S]*)/.exec(section);
    if (!table) continue;
    const dependencyTable = /(?:^|\.)(dependencies|dev-dependencies|build-dependencies)(?:\.["']?([\w-]+)["']?)?$/.exec(table[1]);
    if (!dependencyTable) continue;
    const kind = dependencyTable[1] === "dependencies" ? "normal" : dependencyTable[1];
    const packageName = (body, fallback) => /\bpackage\s*=\s*["']([^"']+)["']/.exec(body)?.[1] ?? fallback;
    if (dependencyTable[2]) {
      edges.push({ name: packageName(table[2], dependencyTable[2]), kind });
    } else {
      for (const entry of table[2].matchAll(/^\s*["']?([\w-]+)["']?\s*=\s*(\{[^}]*\}|[^\n]*)/gm)) {
        edges.push({ name: packageName(entry[2], entry[1]), kind });
      }
    }
  }
  return edges;
}
const cargoDependencies = (text) => cargoEdges(text).map((edge) => edge.name);
const packageNameOf = (text, fallback) =>
  /^\s*\[package\][^[]*?^\s*name\s*=\s*["']([^"']+)["']/m.exec(text.replace(/#.*$/gm, ""))?.[1] ?? fallback;
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
const graph = new Map();
const relative = (file) => path.relative(root, file).replaceAll("\\", "/");
const addToGraph = (manifest, fallback) => {
  const text = fs.readFileSync(manifest, "utf8");
  const name = packageNameOf(text, fallback);
  graph.set(name, { manifest: relative(manifest), edges: cargoEdges(text) });
  return name;
};
for (const base of ["src-tauri", "src-tauri/tools"]) {
  const directory = path.join(root, base);
  if (!fs.existsSync(directory)) continue;
  const manifests = base === "src-tauri"
    ? [path.join(directory, "Cargo.toml")]
    : fs.readdirSync(directory, { withFileTypes: true })
      .filter((entry) => entry.isDirectory())
      .map((entry) => path.join(directory, entry.name, "Cargo.toml"));
  for (const manifest of manifests.filter((file) => fs.existsSync(file))) addToGraph(manifest, path.basename(path.dirname(manifest)));
}
for (const base of ["src-tauri/crates", "crates"]) {
  const directory = path.join(root, base);
  if (!fs.existsSync(directory)) continue;
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const crate = path.join(directory, entry.name);
    const manifest = path.join(crate, "Cargo.toml");
    if (!fs.existsSync(manifest)) continue;
    if (base === "src-tauri/crates") addToGraph(manifest, entry.name);
    const dependencies = cargoDependencies(fs.readFileSync(manifest, "utf8"));
    if (["domios-service-core", "domios-server", `${legacyPrefix}-service-core`].includes(entry.name)) { // rename-bridge
      for (const dependency of dependencies.filter((name) => /^tauri(?:-|$)/.test(name))) {
        violations.push(`${relative(manifest)}: service core depends on ${dependency}`);
      }
    }
    if (hosts.has(entry.name) || isClient(entry.name)) continue;
    for (const dependency of dependencies) {
      if (tierFor(dependency) > tierFor(entry.name)) {
        violations.push(`${relative(manifest)}: leaf tier inversion: ${entry.name} (tier ${tierFor(entry.name)}) -> ${dependency} (tier ${tierFor(dependency)})`);
      }
    }
    const files = [manifest, path.join(crate, "build.rs"), ...sources(path.join(crate, "src")), ...sources(path.join(crate, "tests"))];
    for (const file of files.filter((file) => fs.existsSync(file))) {
      const isManifest = file === manifest;
      const lines = fs.readFileSync(file, "utf8").split(/\r?\n/);
      lines.forEach((raw, index) => {
        const line = raw.replace(isManifest ? /#.*$/ : /\/\/.*$/, "");
        const tokens = isManifest ? line.replace(/\bpath\s*=\s*(?:"(?:\\.|[^"\\])*"|'[^']*')/g, 'path = ""') : line;
        const core = isManifest ? /\b(?:domios|codeterm)[-_]service[-_]core\b/ : /\b(?:domios|codeterm)_service_core\b/; // rename-bridge
        const tauri = isManifest ? /\btauri(?:[-_]\w+)?\b/ : /\btauri(?:_\w+)?\s*::|\b(?:use|extern\s+crate)\s+tauri\b/;
        const location = `${path.relative(root, file).replaceAll("\\", "/")}:${index + 1}`;
        if (/rename-bridge/.test(raw)) return;
        if (!isManifest && /\bServiceContext\b/.test(line)) violations.push(`${location}: leaf crate names ServiceContext`);
        if (core.test(tokens)) violations.push(`${location}: leaf crate names service-core`);
        if (tauri.test(tokens)) {
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

for (const name of serverOnly.filter((name) => layerOf(name) !== "server")) {
  violations.push(`${name}: listed as ${layerOf(name)} layer but must stay in the server layer`);
}
for (const [name, crate] of graph) {
  for (const edge of crate.edges.filter((edge) => isClient(edge.name) && !isClient(name))) {
    violations.push(`${crate.manifest}: ${layerOf(name)} crate ${name} depends on client crate ${edge.name}`);
  }
}
// Normal edges only: a crate links what its own [dependencies] and target dependency tables name.
for (const [name, crate] of graph) {
  const layer = layerOf(name);
  if (layer === "server") continue;
  const allowed = layer === "client" ? ["client", "shared"] : ["shared"];
  const chains = new Map([[name, [name]]]);
  const queue = [name];
  while (queue.length) {
    const current = queue.shift();
    for (const edge of graph.get(current)?.edges ?? []) {
      if (edge.kind !== "normal" || !graph.has(edge.name) || chains.has(edge.name)) continue;
      const chain = [...chains.get(current), edge.name];
      chains.set(edge.name, chain);
      if (allowed.includes(layerOf(edge.name))) {
        queue.push(edge.name);
      } else {
        violations.push(`${crate.manifest}: ${layer} crate ${name} links ${layerOf(edge.name)} crate ${edge.name} (${chain.join(" -> ")})`);
      }
    }
  }
}
if (violations.length) {
  console.error(violations.join("\n"));
  process.exitCode = 1;
} else {
  console.log("crate layering: ok");
}
