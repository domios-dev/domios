#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";

const repoRoot = path.resolve(import.meta.dirname, "..");
const args = process.argv.slice(2);
const check = args.includes("--check");
const versionArg = args.find((arg) => arg !== "--check");
const changed = [];
const rootPackagePath = path.join(repoRoot, "package.json");
const rootPackage = readJson(rootPackagePath);
const version = (versionArg || rootPackage.version || "").trim();

if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
  console.error(`Invalid version: ${version || "(empty)"}`);
  process.exit(1);
}

const providers = ["claude", "codex", "opencode"];
const workspacePackages = ["packages/chat-core", "packages/chat-ui", "packages/plugin-sdk"];

updateJsonVersion(rootPackagePath);
updateJsonVersion(path.join(repoRoot, "src-tauri", "tauri.conf.json"));

updateTomlPackageVersion(path.join(repoRoot, "src-tauri", "Cargo.toml"));
updateTomlPackageVersion(path.join(repoRoot, "cli", "Cargo.toml"));

for (const id of providers) {
  updateJson(path.join(repoRoot, "plugins", `provider-${id}`, "plugin.json"), (json) => {
    json.version = version;
    return json;
  });
  updateJson(path.join(repoRoot, "plugins", `provider-${id}`, "package.json"), (json) => {
    json.version = version;
    return json;
  });
}

for (const packageDir of workspacePackages) {
  updateJson(path.join(repoRoot, packageDir, "package.json"), (json) => {
    json.version = version;
    return json;
  });
}

updateJson(path.join(repoRoot, "plugins", "channel.json"), (json) => {
  for (const plugin of json.plugins ?? []) {
    if (providers.includes(plugin.id)) {
      plugin.version = version;
    }
  }
  return json;
});

if (check) {
  if (changed.length > 0) {
    console.error(`CodeTerm workspace versions are not synced to ${version}:`);
    for (const file of changed) {
      console.error(`  ${path.relative(repoRoot, file)}`);
    }
    process.exit(1);
  }
  console.log(`CodeTerm workspace versions are synced to ${version}`);
} else {
  console.log(`Synced CodeTerm workspace versions to ${version}`);
}

function updateTomlPackageVersion(file) {
  updateText(file, (text) => {
    const next = text.replace(
      /(^\[package\][\s\S]*?^version\s*=\s*")[^"]+(")/m,
      (_match, prefix, suffix) => `${prefix}${version}${suffix}`,
    );
    if (next === text && !text.includes(`version = "${version}"`)) {
      throw new Error(`Could not find [package] version in ${path.relative(repoRoot, file)}`);
    }
    return next;
  });
}

function updateJsonVersion(file) {
  updateText(file, (text) => {
    const next = text.replace(
      /("version"\s*:\s*")[^"]+(")/,
      (_match, prefix, suffix) => `${prefix}${version}${suffix}`,
    );
    if (next === text && !text.includes(`"version": "${version}"`)) {
      throw new Error(`Could not find version field in ${path.relative(repoRoot, file)}`);
    }
    return next;
  });
}

function readJson(file) {
  return JSON.parse(fs.readFileSync(file, "utf8"));
}

function updateJson(file, mutate) {
  updateText(file, (current) => {
    const json = mutate(JSON.parse(current));
    return withOriginalEol(`${JSON.stringify(json, null, 2)}\n`, current);
  });
}

function updateText(file, mutate) {
  const current = fs.readFileSync(file, "utf8");
  const next = mutate(current);
  if (normalizeEol(current) !== normalizeEol(next)) {
    changed.push(file);
    if (!check) {
      fs.writeFileSync(file, next);
    }
  }
}

function withOriginalEol(text, original) {
  return original.includes("\r\n") ? text.replace(/\n/g, "\r\n") : text;
}

function normalizeEol(text) {
  return text.replace(/\r\n/g, "\n");
}
