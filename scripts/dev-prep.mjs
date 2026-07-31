#!/usr/bin/env node
import crypto from "node:crypto";
import {
  accessSync,
  closeSync,
  constants,
  existsSync,
  lstatSync,
  mkdirSync,
  openSync,
  readFileSync,
  readdirSync,
  readlinkSync,
  renameSync,
  rmSync,
  statSync,
  utimesSync,
  watch,
  writeFileSync,
} from "node:fs";
import { spawn, spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const isWin = process.platform === "win32";
const pnpm = isWin ? "pnpm.cmd" : "pnpm";
const cargo = isWin ? "cargo.exe" : "cargo";
const node = process.execPath;
const provenanceSchema = 1;
const inputPathspecs = [
  "build.sh",
  "package.json",
  "pnpm-lock.yaml",
  "pnpm-workspace.yaml",
  "rust-toolchain.toml",
  "index.html",
  "tsconfig.json",
  "tsconfig.node.json",
  "vite.config.ts",
  "src",
  "public",
  "packages",
  "plugins",
  "skills",
  "cli",
  "crates",
  "src-tauri",
  "scripts/dev-prep.mjs",
  "scripts/bundle-cli.js",
  "scripts/bundle-embed.js",
  "scripts/prepare-cli-stub.js",
  "scripts/prepare-embed-stub.js",
  "scripts/dev-resource-preflight.mjs",
  "scripts/dev-static-server.mjs",
  "scripts/convert-plan-storage.rs",
  "scripts/cache-policy.sh",
  "scripts/build-gateway.sh",
  "scripts/release-signing.sh",
];

function run(cmd, args, opts = {}) {
  const res = spawnSync(cmd, args, {
    cwd: repo,
    stdio: "inherit",
    env: process.env,
    shell: isWin && cmd.endsWith(".cmd"),
    ...opts,
  });
  if (res.error) throw res.error;
  if (res.status !== 0) process.exit(res.status ?? 1);
}

function output(cmd, args) {
  const res = spawnSync(cmd, args, {
    cwd: repo,
    encoding: "utf8",
    env: process.env,
    maxBuffer: 64 * 1024 * 1024,
  });
  if (res.error) throw res.error;
  if (res.status !== 0) throw new Error(`${cmd} ${args.join(" ")} failed`);
  return res.stdout.trim();
}

function outputRaw(cmd, args) {
  const res = spawnSync(cmd, args, {
    cwd: repo,
    env: process.env,
    maxBuffer: 64 * 1024 * 1024,
  });
  if (res.error) throw res.error;
  if (res.status !== 0) throw new Error(`${cmd} ${args.join(" ")} failed`);
  return res.stdout;
}

function touch(path) {
  const full = join(repo, path);
  const now = new Date();
  utimesSync(full, now, now);
}

function parseOptions(args) {
  const options = new Map();
  for (let index = 0; index < args.length; index += 1) {
    const key = args[index];
    if (!key.startsWith("--")) throw new Error(`unexpected argument: ${key}`);
    if (index + 1 >= args.length) throw new Error(`${key} requires a value`);
    const next = args[index + 1];
    if (next.startsWith("--")) throw new Error(`${key} requires a value`);
    options.set(key, next);
    index += 1;
  }
  return options;
}

function option(options, key, allowEmpty = false) {
  const value = options.get(key);
  if (typeof value !== "string" || (!allowEmpty && value.length === 0)) throw new Error(`${key} is required`);
  return value;
}

function sha256() {
  return crypto.createHash("sha256");
}

function listTree(root) {
  const files = [];
  const walk = (current) => {
    for (const entry of readdirSync(current, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const path = join(current, entry.name);
      if (entry.isDirectory()) walk(path);
      else files.push(path);
    }
  };
  walk(root);
  return files;
}

function hashPath(requested, expectedType, requireExecutable = false) {
  const path = resolve(requested);
  if (!existsSync(path)) throw new Error(`artifact is absent: ${path}`);
  const stat = lstatSync(path);
  if (expectedType === "file" && !stat.isFile()) throw new Error(`artifact is not a file: ${path}`);
  if (expectedType === "tree" && !stat.isDirectory()) throw new Error(`artifact is not a directory: ${path}`);
  if (requireExecutable && !isWin && (stat.mode & 0o111) === 0) throw new Error(`artifact is not executable: ${path}`);
  const hash = sha256();
  let bytes = 0;
  const files = stat.isDirectory() ? listTree(path) : [path];
  if (files.length === 0) throw new Error(`artifact is empty: ${path}`);
  for (const file of files) {
    const fileStat = lstatSync(file);
    const name = relative(path, file).replaceAll("\\", "/") || ".";
    hash.update(name);
    hash.update("\0");
    if (fileStat.isSymbolicLink()) {
      hash.update(`symlink:${readlinkSync(file)}`);
    } else if (fileStat.isFile()) {
      const content = readFileSync(file);
      bytes += content.length;
      hash.update(content);
    } else {
      throw new Error(`unsupported artifact entry: ${file}`);
    }
    hash.update("\0");
  }
  if (bytes === 0) throw new Error(`artifact has zero bytes: ${path}`);
  return { path, type: expectedType, sha256: hash.digest("hex"), bytes, mode: stat.mode & 0o777 };
}

function inputFingerprint() {
  const listed = outputRaw("git", [
    "ls-files",
    "-z",
    "--cached",
    "--others",
    "--exclude-standard",
    "--",
    ...inputPathspecs,
  ])
    .toString("utf8")
    .split("\0")
    .filter(Boolean)
    .map((path) => path.replaceAll("\\", "/"))
    .sort();
  if (listed.length === 0) throw new Error("no build inputs were found");
  const hash = sha256();
  for (const path of listed) {
    const absolute = join(repo, path);
    hash.update(path);
    hash.update("\0");
    if (!existsSync(absolute)) {
      hash.update("missing\0");
      continue;
    }
    const stat = lstatSync(absolute);
    if (stat.isSymbolicLink()) throw new Error(`build input is a symbolic link: ${path}`);
    if (stat.isFile()) {
      hash.update(readFileSync(absolute));
      hash.update("\0");
    } else throw new Error(`unsupported build input: ${path}`);
  }
  return hash.digest("hex");
}

function gitSha() {
  const sha = output("git", ["rev-parse", "HEAD"]);
  if (!/^[0-9a-f]{40}$/.test(sha)) throw new Error("current Git commit is unavailable");
  return sha;
}

function instanceFrom(options) {
  return {
    name: option(options, "--instance", true),
    apiPort: option(options, "--api-port"),
    vitePort: option(options, "--vite-port"),
    cdpPort: option(options, "--cdp-port"),
    identifier: option(options, "--identifier"),
  };
}

function artifactPaths(options) {
  return {
    app: option(options, "--app"),
    cli: option(options, "--cli"),
    frontend: option(options, "--frontend"),
  };
}

function buildProvenance(options) {
  const expectedSha = option(options, "--build-sha");
  const currentSha = gitSha();
  if (expectedSha !== currentSha) throw new Error(`build SHA mismatch: expected ${expectedSha}, got ${currentSha}`);
  const paths = artifactPaths(options);
  return {
    schema: provenanceSchema,
    buildSha: currentSha,
    inputsSha256: inputFingerprint(),
    instance: instanceFrom(options),
    artifacts: {
      app: hashPath(paths.app, "file", true),
      cli: hashPath(paths.cli, "file", true),
      embed: hashPath(option(options, "--embed"), "file", true),
      frontend: hashPath(paths.frontend, "tree"),
    },
  };
}

function writeProvenance(options) {
  const record = resolve(option(options, "--record"));
  const data = buildProvenance(options);
  mkdirSync(dirname(record), { recursive: true });
  const temporary = `${record}.${process.pid}.tmp`;
  writeFileSync(temporary, `${JSON.stringify(data, null, 2)}\n`, { flag: "wx" });
  rmSync(record, { force: true });
  renameSync(temporary, record);
  console.log(`[dev-provenance] wrote ${record}`);
}

function assertKeys(value, keys, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error(`${label} is not an object`);
  const actual = Object.keys(value).sort();
  const expected = [...keys].sort();
  if (JSON.stringify(actual) !== JSON.stringify(expected)) throw new Error(`${label} has an invalid shape`);
}

function validateArtifact(recorded, expectedPath, expectedType, label, requireExecutable = false) {
  assertKeys(recorded, ["path", "type", "sha256", "bytes", "mode"], `${label} artifact`);
  const expected = resolve(expectedPath);
  if (resolve(recorded.path) !== expected) throw new Error(`${label} artifact path mismatch`);
  if (recorded.type !== expectedType) throw new Error(`${label} artifact type mismatch`);
  const current = hashPath(expected, expectedType, requireExecutable);
  if (recorded.sha256 !== current.sha256 || recorded.bytes !== current.bytes || recorded.mode !== current.mode) {
    throw new Error(`${label} artifact content mismatch`);
  }
}

function validateEmbedArtifact(recorded) {
  assertKeys(recorded, ["path", "type", "sha256", "bytes", "mode"], "embed artifact");
  const expectedRoot = resolve(repo, "src-tauri", "bundled-embed");
  const path = resolve(recorded.path);
  const withinRoot = path.startsWith(`${expectedRoot}${sep}`);
  if (!withinRoot || dirname(path) !== expectedRoot) throw new Error("embed artifact path is outside bundled-embed");
  if (!/^codeterm-embed-[a-zA-Z0-9_.-]+(?:\.exe)?$/.test(path.slice(expectedRoot.length + 1))) {
    throw new Error("embed artifact name is invalid");
  }
  if (recorded.type !== "file") throw new Error("embed artifact type mismatch");
  const current = hashPath(path, "file", true);
  if (recorded.sha256 !== current.sha256 || recorded.bytes !== current.bytes || recorded.mode !== current.mode) {
    throw new Error("embed artifact content mismatch");
  }
}

function checkProvenance(options) {
  const record = resolve(option(options, "--record"));
  if (!existsSync(record)) throw new Error(`record is absent: ${record}`);
  let data;
  try {
    data = JSON.parse(readFileSync(record, "utf8"));
  } catch (error) {
    throw new Error(`record is unparseable: ${error.message}`);
  }
  assertKeys(data, ["schema", "buildSha", "inputsSha256", "instance", "artifacts"], "record");
  if (data.schema !== provenanceSchema) throw new Error(`unsupported record schema: ${data.schema}`);
  if (data.buildSha !== gitSha()) throw new Error("recorded build commit does not match HEAD");
  if (!/^[0-9a-f]{64}$/.test(data.inputsSha256) || data.inputsSha256 !== inputFingerprint()) {
    throw new Error("recorded build inputs are stale");
  }
  const expectedInstance = instanceFrom(options);
  assertKeys(data.instance, ["name", "apiPort", "vitePort", "cdpPort", "identifier"], "instance");
  if (JSON.stringify(data.instance) !== JSON.stringify(expectedInstance)) throw new Error("recorded instance does not match this dev instance");
  assertKeys(data.artifacts, ["app", "cli", "embed", "frontend"], "artifacts");
  const paths = artifactPaths(options);
  validateArtifact(data.artifacts.app, paths.app, "file", "app", true);
  validateArtifact(data.artifacts.cli, paths.cli, "file", "CLI", true);
  validateEmbedArtifact(data.artifacts.embed);
  validateArtifact(data.artifacts.frontend, paths.frontend, "tree", "frontend");
  console.log(`[dev-provenance] matched ${record} at ${data.buildSha}`);
}

function checkRuntime(options) {
  const nodeModules = join(repo, "node_modules");
  const modulesState = join(nodeModules, ".modules.yaml");
  const viteLauncher = join(nodeModules, ".bin", isWin ? "vite.CMD" : "vite");
  const dataDir = resolve(option(options, "--data-dir"));
  const logDir = dirname(resolve(option(options, "--log")));
  if (!existsSync(nodeModules) || !statSync(nodeModules).isDirectory()) throw new Error("node_modules is absent");
  if (!existsSync(modulesState) || !statSync(modulesState).isFile() || statSync(modulesState).size === 0) {
    throw new Error("pnpm dependency state is absent");
  }
  if (!existsSync(viteLauncher) || !statSync(viteLauncher).isFile() || statSync(viteLauncher).size === 0) {
    throw new Error("local Vite launcher is absent");
  }
  if (!isWin) accessSync(viteLauncher, constants.X_OK);
  accessSync(dataDir, constants.R_OK | constants.W_OK);
  accessSync(logDir, constants.R_OK | constants.W_OK);
  const require = createRequire(join(repo, "package.json"));
  for (const dependency of ["vite", "@vitejs/plugin-react", "@tailwindcss/vite", "vite-plugin-pwa", "@tailwindcss/browser"]) {
    require.resolve(dependency);
  }
  console.log("[dev-prebuilt] runtime prerequisites matched");
}

function terminateOwnedChild(child) {
  if (!child || !Number.isInteger(child.pid) || child.exitCode !== null) return;
  if (isWin) {
    const result = spawnSync("taskkill.exe", ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore" });
    if (result.status !== 0) child.kill("SIGKILL");
    return;
  }
  try {
    process.kill(-child.pid, "SIGKILL");
  } catch {
    child.kill("SIGKILL");
  }
}

async function launchPrebuilt(options) {
  const app = resolve(option(options, "--app"));
  const log = resolve(option(options, "--log"));
  const apiPort = option(options, "--api-port");
  const fixtureDeadline = process.env.CODETERM_TEST_PREBUILT_READY_DEADLINE_MS;
  const deadlineMs = fixtureDeadline === undefined ? 30_000 : Number(fixtureDeadline);
  if (!Number.isInteger(deadlineMs) || deadlineMs < 1 || deadlineMs > 30_000) {
    throw new Error("invalid prebuilt readiness failure deadline");
  }
  if (!existsSync(app) || !statSync(app).isFile()) throw new Error(`prebuilt app is absent: ${app}`);
  mkdirSync(dirname(log), { recursive: true });
  writeFileSync(log, "");
  const logHandle = openSync(log, "a");
  let child;
  let watcher;
  let deadline;
  let offset = 0;
  let partial = "";
  try {
    await new Promise((resolveReady, rejectReady) => {
      const fail = (error) => rejectReady(error instanceof Error ? error : new Error(String(error)));
      const inspect = () => {
        try {
          const content = readFileSync(log);
          if (content.length < offset) {
            offset = 0;
            partial = "";
          }
          const added = content.subarray(offset).toString("utf8");
          offset = content.length;
          const lines = `${partial}${added}`.split(/\r?\n/);
          partial = lines.pop() ?? "";
          const signal = lines.find(
            (line) =>
              process.env.CODETERM_TEST_PREBUILT_NEVER_READY !== "1" &&
              line.includes("REST API generation ") &&
              line.includes(" health-proven on http://") &&
              new RegExp(`:${apiPort}(?:\\b|/)`).test(line),
          );
          if (signal) resolveReady(signal);
        } catch (error) {
          fail(error);
        }
      };
      watcher = watch(log, inspect);
      watcher.on("error", fail);
      child = spawn(app, [], {
        cwd: repo,
        detached: true,
        env: process.env,
        stdio: ["ignore", logHandle, logHandle],
        windowsHide: true,
      });
      child.once("error", fail);
      child.once("exit", (code, signal) => fail(new Error(`prebuilt app exited before readiness (code=${code}, signal=${signal})`)));
      child.unref();
      console.log(`[dev-prebuilt] launched pid ${child.pid}, log ${log}`);
      deadline = setTimeout(() => fail(new Error(`prebuilt app did not report readiness within ${deadlineMs}ms`)), deadlineMs);
    }).then((signal) => console.log(`[dev-prebuilt] readiness signal: ${signal}`));
  } catch (error) {
    terminateOwnedChild(child);
    throw error;
  } finally {
    if (deadline) clearTimeout(deadline);
    if (watcher) watcher.close();
    if (child) child.removeAllListeners();
    closeSync(logHandle);
  }
}

function runRegularPrep() {
  const startServer = !process.argv.includes("--no-server");
  const host = output(cargo, ["-vV"])
    .split(/\r?\n/)
    .find((line) => line.startsWith("host:"))
    ?.split(":")[1]
    ?.trim();

  if (!host) {
    console.error("[dev-prep] failed to detect cargo host triple");
    process.exit(1);
  }

  const embedBundle = join(repo, "src-tauri", "bundled-embed", `codeterm-embed-${host}${isWin ? ".exe" : ""}`);
  touch("cli/src/main.rs");
  run(cargo, ["build", "--release", "--manifest-path", "cli/Cargo.toml"]);
  process.env.CODETERM_DEV = "1";
  run(node, ["scripts/bundle-cli.js"]);
  run(node, ["scripts/prepare-embed-stub.js"]);

  const embedSize = existsSync(embedBundle) ? statSync(embedBundle).size : 0;
  if (process.env.REBUILD_EMBED === "1" || embedSize < 1_048_576) {
    console.log(`[dev-prep] building codeterm-embed (size=${embedSize}, REBUILD_EMBED=${process.env.REBUILD_EMBED ?? "0"})`);
    run(cargo, ["build", "--release", "--manifest-path", "src-tauri/Cargo.toml", "--bin", "codeterm-embed"]);
    run(node, ["scripts/bundle-embed.js"]);
  } else {
    console.log(`[dev-prep] skipping codeterm-embed rebuild (bundle exists, ${embedSize} bytes; set REBUILD_EMBED=1 to force)`);
  }

  if (startServer) run(node, ["scripts/dev-static-server.mjs"]);
}

const command = process.argv[2];
try {
  if (command === "--write-provenance") writeProvenance(parseOptions(process.argv.slice(3)));
  else if (command === "--check-provenance") checkProvenance(parseOptions(process.argv.slice(3)));
  else if (command === "--check-runtime") checkRuntime(parseOptions(process.argv.slice(3)));
  else if (command === "--launch-prebuilt") await launchPrebuilt(parseOptions(process.argv.slice(3)));
  else runRegularPrep();
} catch (error) {
  console.error(`[dev-prep] ${error.message}`);
  process.exit(1);
}
