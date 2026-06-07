#!/usr/bin/env bash
set -euo pipefail

name="${1:?usage: bump.sh <name> <plugin-id> <new-version>}"
plugin_id="${2:?usage: bump.sh <name> <plugin-id> <new-version>}"
new_version="${3:?usage: bump.sh <name> <plugin-id> <new-version>}"
root="${CODETERM_TEST_CHANNEL_ROOT:-/tmp/codeterm-test-channels}"
work="$root/$name"

test -d "$work/.git" || {
  echo "channel worktree not found: $work" >&2
  exit 2
}

node - "$work/plugins/channel.json" "$plugin_id" "$new_version" <<'NODE'
const fs = require("fs");
const [path, id, version] = process.argv.slice(2);
const manifest = JSON.parse(fs.readFileSync(path, "utf8"));
const plugin = manifest.plugins.find((p) => p.id === id);
if (!plugin) throw new Error(`plugin ${id} not found in ${path}`);
plugin.version = version;
fs.writeFileSync(path, JSON.stringify(manifest, null, 2) + "\n");
NODE

plugin_path="$(node - "$work/plugins/channel.json" "$plugin_id" <<'NODE'
const fs = require("fs");
const [path, id] = process.argv.slice(2);
const manifest = JSON.parse(fs.readFileSync(path, "utf8"));
const plugin = manifest.plugins.find((p) => p.id === id);
if (!plugin) throw new Error(`plugin ${id} not found in ${path}`);
console.log(plugin.path || plugin.source);
NODE
)"

node - "$work/plugins/$plugin_path/plugin.json" "$new_version" <<'NODE'
const fs = require("fs");
const [path, version] = process.argv.slice(2);
const manifest = JSON.parse(fs.readFileSync(path, "utf8"));
manifest.version = version;
fs.writeFileSync(path, JSON.stringify(manifest, null, 2) + "\n");
NODE

git -C "$work" add plugins
git -C "$work" commit -m "bump $plugin_id to $new_version" >/dev/null
git -C "$work" push origin main >/dev/null
