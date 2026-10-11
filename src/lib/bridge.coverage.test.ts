import fs from "node:fs";
import path from "node:path";
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { createCommandRegistry, CANNED_COMMANDS, REFUSED_COMMANDS } from "./transportCommands";
import { NATIVE_COMMANDS } from "./nativeCommands";
import type { TransportClient } from "./transportClient";
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
function files(directory: string): string[] {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
    if (["node_modules", "dist"].includes(entry.name)) return [];
    const file = path.join(directory, entry.name);
    return entry.isDirectory() ? files(file) : /\.tsx?$/.test(file) && !/\.(test|testdata)\./.test(file) ? [file] : [];
  });
}
const client: TransportClient = { json: async <T>() => ({} as T), localJson: async <T>() => ({} as T), currentToken: () => null, origin: () => "http://fixture", upload: async () => { throw new Error("Unexpected upload"); }, subscribe: () => () => {} };
const known = new Set([...Object.keys(createCommandRegistry(client)), ...Object.keys(NATIVE_COMMANDS), ...Object.keys(CANNED_COMMANDS), ...Object.keys(REFUSED_COMMANDS)]);
const missing = new Set<string>();
for (const file of files(path.join(root, "src"))) {
  for (const match of fs.readFileSync(file, "utf8").matchAll(/invoke(?:<[^>]*>)?\(\s*["']([a-z_][a-z0-9_]*)["']/g)) {
    if (!known.has(match[1])) missing.add(match[1]);
  }
}
assert.deepEqual([...missing].sort(), []);
console.log("Literal bridge commands resolve through the named registry or native capabilities");
