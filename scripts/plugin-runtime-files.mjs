import fs from "node:fs";
import path from "node:path";

export function runtimeFileAllowed(relative, rules) {
  if (relative.split("/").some(part => part.startsWith(".") ||
      ["src", "tests", "fixtures", "node_modules", "target"].includes(part)) ||
      relative.includes(".test.") || /\.(ts|tsx|map)$/.test(relative)) return false;
  return rules.some(rule => relative === rule || (rule.endsWith("/") && relative.startsWith(rule)));
}

export function runtimeFiles(directory) {
  const rules = fs.readFileSync(path.join(directory, "runtime-files.txt"), "utf8").split(/\r?\n/).map(s => s.trim()).filter(Boolean);
  const files = [];
  function walk(current, prefix = "") {
    for (const entry of fs.readdirSync(current, { withFileTypes: true })) {
      const relative = `${prefix}${entry.name}`;
      if (entry.isSymbolicLink()) throw new Error(`symlink in plugin: ${relative}`);
      if (entry.isDirectory() && !entry.name.startsWith(".") && !["node_modules", "target"].includes(entry.name)) walk(path.join(current, entry.name), `${relative}/`);
      else if (entry.isFile() && runtimeFileAllowed(relative, rules)) files.push(relative);
    }
  }
  walk(directory);
  const manifest = JSON.parse(fs.readFileSync(path.join(directory, "plugin.json"), "utf8"));
  for (const required of ["plugin.json", "runtime-files.txt", manifest.entry, manifest.icon, manifest.agentGuide, manifest.settingsSchema].filter(value => typeof value === "string")) {
    if (!files.includes(required)) throw new Error(`runtime allowlist omits ${required}`);
  }
  return files.sort();
}

export function archiveRuntimeFiles(sourceSha, subdirectory, output, run) {
  const tree = `${sourceSha}:${subdirectory}`;
  const entries = run("git", ["ls-tree", "-r", tree]).toString().trim().split("\n").filter(Boolean).map(line => {
    const [metadata, name] = line.split("\t");
    return { mode: metadata.split(" ")[0], name };
  });
  const names = entries.map(entry => entry.name);
  const pluginDirectories = subdirectory === "plugins" ? names.filter(name => name.endsWith("/plugin.json")).map(name => name.slice(0, -12)) : [""];
  const selected = subdirectory === "plugins" ? ["channel.json"] : [];
  for (const directory of pluginDirectories) {
    const prefix = directory ? `${directory}/` : "";
    const rules = run("git", ["show", `${tree}/${prefix}runtime-files.txt`]).toString().split(/\r?\n/).map(s => s.trim()).filter(Boolean);
    selected.push(...names.filter(name => name.startsWith(prefix) && runtimeFileAllowed(name.slice(prefix.length), rules)));
  }
  if (!selected.length) throw new Error("empty runtime plugin archive");
  if (entries.some(entry => selected.includes(entry.name) && entry.mode !== "100644" && entry.mode !== "100755")) throw new Error("non-regular file in runtime plugin archive");
  run("git", ["archive", "--format=tar.gz", `--output=${output}`, tree, "--", ...selected]);
}
