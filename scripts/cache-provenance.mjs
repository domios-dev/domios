#!/usr/bin/env node
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

function run(cmd, args) {
  try {
    return execFileSync(cmd, args, { cwd: repo, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();
  } catch {
    return "";
  }
}

function runRaw(cmd, args) {
  try {
    return execFileSync(cmd, args, { cwd: repo, stdio: ["ignore", "pipe", "ignore"] });
  } catch {
    return Buffer.alloc(0);
  }
}

function shaFile(file) {
  if (!fs.existsSync(file) || !fs.statSync(file).isFile()) return null;
  return crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}

function shaText(text) {
  return crypto.createHash("sha256").update(text).digest("hex");
}

function listFiles(dir) {
  if (!fs.existsSync(dir)) return [];
  const out = [];
  const walk = (base) => {
    for (const entry of fs.readdirSync(base, { withFileTypes: true })) {
      const full = path.join(base, entry.name);
      if (entry.isDirectory()) walk(full);
      else if (entry.isFile()) out.push(full);
    }
  };
  walk(dir);
  return out.sort();
}

function hashDir(dir) {
  const files = listFiles(dir);
  if (files.length === 0) return null;
  const h = crypto.createHash("sha256");
  for (const file of files) {
    h.update(path.relative(repo, file).replaceAll("\\", "/"));
    h.update("\0");
    h.update(fs.readFileSync(file));
    h.update("\0");
  }
  return h.digest("hex");
}

function relativeRepoPath(file) {
  if (!file) return null;
  const relative = path.relative(repo, path.resolve(repo, file)).replaceAll("\\", "/");
  return relative.startsWith("../") || relative === ".." ? null : relative;
}

function workingTreeHash(excludes) {
  const files = runRaw("git", ["ls-files", "--cached", "--others", "--exclude-standard", "-z"])
    .toString("utf8")
    .split("\0")
    .filter(Boolean)
    .sort();
  const hash = crypto.createHash("sha256");
  for (const relative of files) {
    const normalized = relative.replaceAll("\\", "/");
    if (excludes.has(normalized)) continue;
    const absolute = path.join(repo, relative);
    hash.update(normalized);
    hash.update("\0");
    if (!fs.existsSync(absolute)) {
      hash.update("missing\0");
      continue;
    }
    const stat = fs.lstatSync(absolute);
    if (stat.isSymbolicLink()) {
      hash.update(`symlink:${fs.readlinkSync(absolute)}\0`);
    } else if (stat.isFile()) {
      hash.update(`mode:${stat.mode & 0o777}\0`);
      hash.update(fs.readFileSync(absolute));
      hash.update("\0");
    }
  }
  return hash.digest("hex");
}

function sourceState(excludedFiles) {
  const trackedChanges = run("git", ["diff", "--no-ext-diff", "--binary", "HEAD", "--"]);
  const untrackedFiles = run("git", ["ls-files", "--others", "--exclude-standard"]);
  const status = [trackedChanges, untrackedFiles].filter(Boolean).join("\n");
  const excludes = new Set(excludedFiles.map(relativeRepoPath).filter(Boolean));
  return {
    git_sha: run("git", ["rev-parse", "HEAD"]),
    git_status_sha: shaText(status),
    working_tree_sha: workingTreeHash(excludes),
    clean: status.length === 0,
  };
}

function generatedState(kind) {
  if (kind === "full-build-input") return {};
  const state = {
    dist: hashDir(path.join(repo, "dist")),
    schemas: hashDir(path.join(repo, "src-tauri", "gen", "schemas")),
  };
  if (kind === "full-build") {
    state.bundled_cli = hashDir(path.join(repo, "src-tauri", "bundled-cli"));
    state.bundled_embed = hashDir(path.join(repo, "src-tauri", "bundled-embed"));
  }
  return state;
}

function currentFingerprint(opts, excludedFiles = []) {
  return {
    schema: 1,
    kind: opts.kind,
    source: sourceState(excludedFiles),
    profile: opts.profile,
    target: opts.target,
    toolchain: {
      rustc: opts.toolchain_fixture ?? run("rustc", ["-vV"]),
      cargo: opts.toolchain_fixture ?? run("cargo", ["-V"]),
      node: opts.toolchain_fixture ?? run("node", ["--version"]),
      pnpm: opts.toolchain_fixture ?? run("pnpm", ["--version"]),
    },
    flags: opts.flags,
    features: opts.features,
    generated: generatedState(opts.kind),
  };
}

function parseArgs(argv) {
  const opts = {};
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (!arg.startsWith("--")) continue;
    opts[arg.slice(2).replaceAll("-", "_")] = argv[i + 1];
    i += 1;
  }
  return opts;
}

function write(opts) {
  const artifact = path.resolve(repo, opts.artifact);
  const data = {
    ...currentFingerprint(opts, [opts.artifact, opts.out]),
    artifact: {
      path: path.relative(repo, artifact).replaceAll("\\", "/"),
      sha256: shaFile(artifact),
      bytes: fs.existsSync(artifact) ? fs.statSync(artifact).size : 0,
    },
  };
  fs.mkdirSync(path.dirname(path.resolve(repo, opts.out)), { recursive: true });
  fs.writeFileSync(path.resolve(repo, opts.out), `${JSON.stringify(data, null, 2)}\n`);
  console.log(`[provenance] wrote ${opts.out}`);
}

function snapshot(opts) {
  const data = currentFingerprint(opts, [opts.out]);
  fs.mkdirSync(path.dirname(path.resolve(repo, opts.out)), { recursive: true });
  fs.writeFileSync(path.resolve(repo, opts.out), `${JSON.stringify(data, null, 2)}\n`);
  console.log(`[provenance] wrote input snapshot ${opts.out}`);
}

function check(opts) {
  const manifest = JSON.parse(fs.readFileSync(path.resolve(repo, opts.manifest), "utf8"));
  const current = currentFingerprint(opts, [opts.manifest, manifest.artifact?.path]);
  const keys = ["kind", "profile", "target", "flags", "features"];
  for (const key of keys) {
    if (JSON.stringify(manifest[key]) !== JSON.stringify(current[key])) {
      throw new Error(`${key} mismatch`);
    }
  }
  if (JSON.stringify(manifest.source) !== JSON.stringify(current.source)) {
    throw new Error("source SHA/status mismatch");
  }
  if (JSON.stringify(manifest.toolchain) !== JSON.stringify(current.toolchain)) {
    throw new Error("toolchain mismatch");
  }
  if (JSON.stringify(manifest.generated) !== JSON.stringify(current.generated)) {
    throw new Error("generated-resource fingerprint mismatch");
  }
  if (manifest.artifact?.path) {
    const artifact = path.resolve(repo, manifest.artifact.path);
    if (shaFile(artifact) !== manifest.artifact.sha256) {
      throw new Error("artifact hash mismatch");
    }
    if (!fs.existsSync(artifact) || fs.statSync(artifact).size !== manifest.artifact.bytes) {
      throw new Error("artifact size mismatch");
    }
  }
  console.log(`[provenance] matched ${opts.manifest}`);
}

const [cmd, ...rest] = process.argv.slice(2);
const opts = parseArgs(rest);
try {
  if (cmd === "write") write(opts);
  else if (cmd === "snapshot") snapshot(opts);
  else if (cmd === "check") check(opts);
  else throw new Error("usage: cache-provenance.mjs write|snapshot|check --kind K --profile P --target T --flags F --features F");
} catch (err) {
  console.error(`[provenance] ${err.message}`);
  process.exit(1);
}
