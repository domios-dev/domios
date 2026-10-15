// Caller: scripts check-plugin.mjs, ci-scope.mjs, release-plugin.mjs, sync-plugin-versions.mjs
import fs from "node:fs";
import path from "node:path";

// Workspace crates released at the app version; every other crate keeps its own.
export const APP_VERSIONED_CRATES = ["codeterm-service-core", "codeterm-api", "codeterm-socket", "codeterm-commands", "codeterm-sentinel-host", "codeterm-orchestration"];

const versionPattern = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/;

export function parseVersion(value) {
  const match = typeof value === "string" && value.match(versionPattern);
  if (!match || match[4]?.split(".").some((part) => !part || (/^\d+$/.test(part) && part.length > 1 && part[0] === "0"))) {
    throw new Error(`Invalid semantic version: ${value}`);
  }
  return [...match.slice(1, 4).map(Number), match[4]?.split(".") ?? []];
}

export function compareVersions(left, right) {
  const a = parseVersion(left);
  const b = parseVersion(right);
  for (let index = 0; index < 3; index++) {
    if (a[index] !== b[index]) return a[index] > b[index] ? 1 : -1;
  }
  if (!a[3].length || !b[3].length) return Number(!a[3].length) - Number(!b[3].length);
  for (let index = 0; index < Math.max(a[3].length, b[3].length); index++) {
    const first = a[3][index];
    const second = b[3][index];
    if (first === second) continue;
    if (first === undefined || second === undefined) return first === undefined ? -1 : 1;
    const numericA = /^\d+$/.test(first);
    const numericB = /^\d+$/.test(second);
    if (numericA !== numericB) return numericA ? -1 : 1;
    return (numericA ? Number(first) > Number(second) : first > second) ? 1 : -1;
  }
  return 0;
}

function readJson(file) {
  return JSON.parse(fs.readFileSync(file, "utf8"));
}

function writeJson(file, value) {
  const original = fs.readFileSync(file, "utf8");
  const text = `${JSON.stringify(value, null, 2)}\n`;
  fs.writeFileSync(file, original.includes("\r\n") ? text.replaceAll("\n", "\r\n") : text);
}

export function channelPlugins(root) {
  const channel = readJson(path.join(root, "plugins/channel.json"));
  const seen = new Set();
  return channel.plugins.map((entry) => {
    if (!/^[a-z][a-z0-9-]*$/.test(entry.id) || seen.has(entry.id)) {
      throw new Error(`Invalid or duplicate plugin id: ${entry.id}`);
    }
    seen.add(entry.id);
    if (!/^[a-z][a-z0-9-]*$/.test(entry.path)) throw new Error(`Invalid plugin path: ${entry.path}`);
    const directory = path.join(root, "plugins", entry.path);
    const manifest = readJson(path.join(directory, "plugin.json"));
    const packageJson = readJson(path.join(directory, "package.json"));
    parseVersion(manifest.version);
    parseVersion(manifest.minCodeterm);
    if (entry.id !== manifest.id || entry.version !== manifest.version ||
        entry.minCodeterm !== manifest.minCodeterm || packageJson.version !== manifest.version) {
      throw new Error(`Plugin metadata disagrees for ${entry.id}: manifest, package and channel must match`);
    }
    return { ...entry, directory, manifest, packageJson };
  });
}

export function bumpPlugin(root, id, version, minimum) {
  const plugins = channelPlugins(root);
  const plugin = plugins.find((entry) => entry.id === id);
  if (!plugin) throw new Error(`Plugin is not in the official channel: ${id}`);
  if (compareVersions(version, plugin.version) <= 0) throw new Error(`Plugin version must increase: ${plugin.version} -> ${version}`);
  if (minimum !== undefined) parseVersion(minimum);
  const minCodeterm = minimum ?? plugin.minCodeterm;
  const channelPath = path.join(root, "plugins/channel.json");
  const channel = readJson(channelPath);
  const entry = channel.plugins.find((entry) => entry.id === id);
  Object.assign(entry, { version, minCodeterm });
  writeJson(path.join(plugin.directory, "plugin.json"), { ...plugin.manifest, version, minCodeterm });
  writeJson(path.join(plugin.directory, "package.json"), { ...plugin.packageJson, version });
  writeJson(channelPath, channel);
}

export function checkPluginTag(root, tag) {
  const match = /^plugin-([a-z][a-z0-9-]*)-v(.+)$/.exec(tag);
  if (!match) throw new Error(`Invalid plugin release tag: ${tag}`);
  const plugin = channelPlugins(root).find((entry) => entry.id === match[1]);
  if (!plugin || plugin.version !== match[2]) throw new Error(`Tag does not match the published plugin metadata: ${tag}`);
  return plugin;
}
