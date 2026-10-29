#!/usr/bin/env node
// Caller: build.sh gateway-fake|gateway-sleep|gateway-status|gateway-cancel|gateway-policy-probe|gateway-log-probe/dev-prebuilt; scripts tests/release-input-reuse.test.mjs
import { devResourceRoot, devServerPath } from "./dev-native-layout.mjs";
import crypto from "node:crypto";
import { StringDecoder } from "node:string_decoder";
import { runDevApp, nativeLocatorRecord, prebuiltReadinessPhase } from "./dev-app-process.mjs";
import { devRuntimeEnvironment } from "./dev-runtime-environment.mjs";
import { retainLaunchOwner } from "./dev-launch-owner.mjs";
import { waitForReady } from "./server-readiness.mjs";
import {
  accessSync,
  constants,
  existsSync,
  lstatSync,
  mkdirSync,
  openSync,
  readFileSync,
  readdirSync,
  readlinkSync,
  realpathSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { spawn, spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repo = realpathSync.native(resolve(dirname(fileURLToPath(import.meta.url)), ".."));
const require = createRequire(import.meta.url);
const { gitOutput } = require("./build-identity.cjs");
const { reloadPlan, inputSnapshot, cliSourceRoots } = require("./dev-reload-inputs.cjs");
const isWin = process.platform === "win32";
const pnpm = isWin ? "pnpm.cmd" : "pnpm";
const cargo = isWin ? "cargo.exe" : "cargo";
const node = process.execPath;
const provenanceSchema = 3;
const childExitDeadlineMs = 10_000;
const inputPathspecs = [
  "build.sh",
  "package.json",
  "pnpm-lock.yaml",
  "pnpm-workspace.yaml",
  "rust-toolchain.toml",
  "index.html",
  "tsconfig.json",
  "tsconfig.node.json",
  "vite.config.mts",
  "src",
  "public",
  "packages",
  "plugins",
  "skills",
  "cli",
  "crates",
  "src-tauri",
  "scripts/dev-prep.mjs",
  "scripts/local-build-env.mjs",
  "scripts/local-build-env.sh",
  "scripts/firebase-build-config.sh",
  "scripts/dev-reload-inputs.cjs",
  "scripts/dev-server.mjs",
  "scripts/dev-rebuild-stop.mjs",
  "scripts/dev-instance-processes.mjs",
  "scripts/unix-dev-instance-processes.py",
  "scripts/dev-launch-owner.mjs",
  "scripts/dev-launch-file-lock.mjs",
  "scripts/dev-launch-file-lock.py",
  "scripts/windows-dev-launch-file-lock.ps1",
  "scripts/windows-dev-rebuild-stop.ps1",
  "scripts/windows-dev-rebuild-process.cs",
  "scripts/dev-server-bootstrap.ps1",
  "scripts/server-artifact.mjs",
  "scripts/dev-macos-bundle.mjs",
  "scripts/dev-native-layout.mjs",
  "scripts/package-server-component-manifest.mjs",
  "scripts/dev-app-process.mjs",
  "scripts/dev-runtime-environment.mjs",
  "scripts/dev-instance-policy.mjs",
  "scripts/windows-dev-app.ps1",
  "scripts/windows-dev-app.cs",
  "scripts/windows-dev-desktop.cs",
  "scripts/bundle-cli.js",
  "scripts/build-artifacts.cjs",
  "scripts/frontend-build.mjs",
  "scripts/prepare-cli-stub.js",
  "scripts/dev-resource-preflight.mjs",
  "scripts/dev-static-server.mjs",
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

function artifactPath(requested) {
  return realpathSync.native(resolve(requested));
}

function hashPath(requested, expectedType, requireExecutable = false) {
  const path = artifactPath(requested);
  if (!existsSync(path)) throw new Error(`artifact is absent: ${path}`);
  const stat = lstatSync(path);
  if (expectedType === "file" && !stat.isFile()) throw new Error(`artifact is not a file: ${path}`);
  if (expectedType === "tree" && !stat.isDirectory()) throw new Error(`artifact is not a directory: ${path}`);
  if (requireExecutable && !isWin && (stat.mode & 0o111) === 0) throw new Error(`artifact is not executable: ${path}`);
  const hash = sha256();
  let bytes = 0;
  const files = expectedType === "runtime-resources"
    ? ["cli", "skills", "LICENSE", "NOTICE", ...(isWin ? ["conpty"] : [])].flatMap(name => {
        const resource = join(path, name);
        const metadata = lstatSync(resource);
        const entries = metadata.isDirectory() ? listTree(resource) : [resource];
        if (metadata.isSymbolicLink() || entries.some(file => lstatSync(file).isSymbolicLink())) throw new Error("Runtime resource links are not accepted");
        return entries;
      }).sort()
    : stat.isDirectory() ? listTree(path) : [path];
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

function inputFiles() {
  return gitOutput([
    "ls-files",
    "-z",
    "--cached",
    "--others",
    "--exclude-standard",
    "--",
    ...inputPathspecs,
  ], { cwd: repo })
    .split("\0")
    .filter(Boolean)
    .map((path) => path.replaceAll("\\", "/"))
    .sort();
}

function inputFingerprint() {
  const listed = inputFiles();
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
  const sha = gitOutput(["rev-parse", "HEAD"], { cwd: repo });
  if (!/^[0-9a-f]{40}$/.test(sha)) throw new Error("current Git commit is unavailable");
  return sha;
}

function provenanceSha(options) {
  if (!options.has("--launch-sha")) return gitSha();
  const sha = option(options, "--launch-sha");
  if (!/^[0-9a-f]{40}$/.test(sha)) throw new Error("invalid launch SHA");
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
    server: devServerPath(option(options, "--app")),
    inventory: join(devResourceRoot(option(options, "--app")), "server-component.json"),
    resources: devResourceRoot(option(options, "--app")),
  };
}

function buildProvenance(options) {
  const expectedSha = option(options, "--build-sha");
  const currentSha = provenanceSha(options);
  if (expectedSha !== currentSha) throw new Error(`build SHA mismatch: expected ${expectedSha}, got ${currentSha}`);
  const paths = artifactPaths(options);
  return {
    schema: provenanceSchema,
    buildSha: currentSha,
    inputsSha256: inputFingerprint(),
    reload: { inputs: inputSnapshot(repo, inputFiles()), cliRoots: cliSourceRoots(repo) },
    instance: instanceFrom(options),
    artifacts: {
      app: hashPath(paths.app, "file", true),
      cli: hashPath(paths.cli, "file", true),
      frontend: hashPath(paths.frontend, "tree"),
      server: hashPath(paths.server, "file", true),
      inventory: hashPath(paths.inventory, "file"),
      resources: hashPath(paths.resources, "runtime-resources"),
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
  const expected = artifactPath(expectedPath);
  if (artifactPath(recorded.path) !== expected) throw new Error(`${label} artifact path mismatch`);
  if (recorded.type !== expectedType) throw new Error(`${label} artifact type mismatch`);
  const current = hashPath(expected, expectedType, requireExecutable);
  if (recorded.sha256 !== current.sha256 || recorded.bytes !== current.bytes || recorded.mode !== current.mode) {
    throw new Error(`${label} artifact content mismatch`);
  }
}

function checkProvenance(options, serverReload = false) {
  const record = resolve(option(options, "--record"));
  if (!existsSync(record)) throw new Error(`record is absent: ${record}`);
  let data;
  try {
    data = JSON.parse(readFileSync(record, "utf8"));
  } catch (error) {
    throw new Error(`record is unparseable: ${error.message}`);
  }
  const {nativeClient, reload, ...buildRecord} = data;
  assertKeys(buildRecord, ["schema", "buildSha", "inputsSha256", "instance", "artifacts"], "record");
  if (data.schema !== provenanceSchema) throw new Error(`unsupported record schema: ${data.schema}`);
  if (!serverReload && data.buildSha !== provenanceSha(options)) throw new Error("recorded build commit does not match HEAD or launch SHA");
  const fingerprint = inputFingerprint();
  if (!/^[0-9a-f]{64}$/.test(data.inputsSha256) || (!serverReload && data.inputsSha256 !== fingerprint)) {
    throw new Error("recorded build inputs are stale");
  }
  const expectedInstance = instanceFrom(options);
  assertKeys(data.instance, ["name", "apiPort", "vitePort", "cdpPort", "identifier"], "instance");
  if (JSON.stringify(data.instance) !== JSON.stringify(expectedInstance)) throw new Error("recorded instance does not match this dev instance");
  assertKeys(data.artifacts, ["app", "cli", "frontend", "server", "inventory", "resources"], "artifacts");
  const paths = artifactPaths(options);
  validateArtifact(data.artifacts.app, paths.app, "file", "app", true);
  validateArtifact(data.artifacts.cli, paths.cli, "file", "CLI", true);
  if (!serverReload) validateArtifact(data.artifacts.frontend, paths.frontend, "tree", "frontend");
  validateArtifact(data.artifacts.server, paths.server, "file", "server", true);
  validateArtifact(data.artifacts.inventory, paths.inventory, "file", "inventory");
  validateArtifact(data.artifacts.resources, paths.resources, "runtime-resources", "resources");
  if (serverReload) {
    const plan = reloadPlan(reload?.inputs, inputSnapshot(repo, inputFiles()), reload?.cliRoots ?? []);
    console.log(`${plan.cli ? 'cli' : 'server'}:${fingerprint}`);
  } else console.log(`[dev-provenance] matched ${record} at ${data.buildSha}`);
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

async function terminateOwnedChild(child) {
  if (!child || !Number.isInteger(child.pid) || child.exitCode !== null || child.signalCode !== null) return;
  child.ref();
  let deadline;
  let onExit;
  const exited = new Promise((resolveExit, rejectExit) => {
    onExit = resolveExit;
    child.once("exit", onExit);
    deadline = setTimeout(
      () => rejectExit(new Error(`prebuilt app pid ${child.pid} did not exit within ${childExitDeadlineMs}ms`)),
      childExitDeadlineMs,
    );
  });
  try {
    if (isWin) {
      const result = spawnSync("taskkill.exe", ["/PID", String(child.pid), "/T", "/F"], {
        stdio: "ignore",
        timeout: childExitDeadlineMs,
      });
      if (result.status !== 0) child.kill("SIGKILL");
    } else {
      try {
        process.kill(-child.pid, "SIGKILL");
      } catch {
        child.kill("SIGKILL");
      }
    }
    await exited;
  } finally {
    if (deadline) clearTimeout(deadline);
    if (onExit) child.off("exit", onExit);
    child.unref();
  }
}

function readinessSignal(line, apiPort) {
  if (process.env.CODETERM_TEST_PREBUILT_NEVER_READY === "1") return false;
  const prefix = "DOMIOS_DEV_ATTACHMENT ";
  if (!line.startsWith(prefix)) return false;
  try {
    const status = JSON.parse(line.slice(prefix.length));
    return Object.keys(status).sort().join(",") === "api_address,client_pid,epoch,protocol,server_pid"
      && status.protocol === 2 && Number.isSafeInteger(status.client_pid) && status.client_pid > 0
      && Number.isSafeInteger(status.server_pid) && status.server_pid > 0 && status.client_pid !== status.server_pid
      && typeof status.epoch === "string" && /^[a-f0-9-]{36}$/.test(status.epoch)
      && [`127.0.0.1:${apiPort}`, `[::1]:${apiPort}`].includes(status.api_address);
  } catch { return false; }
}

function startupFailureSignal(line) {
  const prefix = "ERROR: DOMIOS_DEV_FAILURE ";
  if (!line.startsWith(prefix)) return null;
  try {
    const value = JSON.parse(line.slice(prefix.length));
    const stages = ["resource_root", "inventory", "install", "resource_verification", "server_artifact", "client_artifact", "identity_resolution", "attach", "launch", "reply_validation", "api_attachment", "native_enrollment", "frontend", "startup"];
    if (Object.keys(value).sort().join(",") !== "exit_code,io_kind,os_error,stage,status"
      || !stages.includes(value.stage) || value.status !== "refused"
      || value.io_kind !== null || value.os_error !== null || value.exit_code !== null) return null;
    return `Native client startup refused at ${value.stage}`;
  } catch { return null; }
}

function attachmentProof(lines, port, listenerPid, alive) {
  const events = lines.filter(line => readinessSignal(line, port));
  if (!events.length) throw new Error("Native attachment readiness is absent");
  const event = JSON.parse(events.at(-1).slice("DOMIOS_DEV_ATTACHMENT ".length));
  if (!/^[1-9][0-9]*$/.test(String(listenerPid)) || event.server_pid !== Number(listenerPid)) {
    throw new Error("Native attachment differs from the actual DEV API listener");
  }
  if (!alive(event.client_pid) || !alive(event.server_pid)) {
    throw new Error("Native attachment client or server is no longer live");
  }
  return event;
}

function checkAttachment(options) {
  const log = resolve(option(options, "--log"));
  const stat = lstatSync(log);
  if (!stat.isFile() || stat.size > 4 * 1024 * 1024) throw new Error("Native attachment log exceeds its bound");
  const port = option(options, "--api-port");
  attachmentProof(readFileSync(log, "utf8").split(/\r?\n/), port, option(options, "--listener-pid"), pid => {
    try { process.kill(pid, 0); return true; } catch { return false; }
  });
}

function disconnectSupervisor() {
  if (process.connected) process.disconnect();
}

function nativeLocatorSink(options) {
  if (!options.has("--record")) return () => {};
  const file=resolve(option(options,"--record")), app=realpathSync.native(option(options,"--app"));
  let partial="", attachment=null, pending=null;
  const decoder=new StringDecoder("utf8");
  const write = data => {
    const temporary=`${file}.${process.pid}.locator.tmp`;
    writeFileSync(temporary,JSON.stringify(data,null,2)+"\n",{flag:"wx"});
    renameSync(temporary,file);
  };
  const initial=JSON.parse(readFileSync(file,"utf8"));
  write({...initial,nativeClient:null});
  return chunk => {
    const lines=(partial+decoder.write(chunk)).split(/\r?\n/); partial=lines.pop();
    if (partial.length > 65536) throw Error("Native DEV diagnostic line exceeds bound");
    for (const line of lines) {
      if (readinessSignal(line,option(options,"--api-port"))) attachment=JSON.parse(line.slice("DOMIOS_DEV_ATTACHMENT ".length));
      if (line.startsWith("DOMIOS_DEV_NATIVE_LOCATOR ")) pending=line;
      if (!attachment || !pending) continue;
      const record=JSON.parse(readFileSync(file,"utf8"));
      const next=nativeLocatorRecord(pending,record,app,attachment);
      if (next) write(next);
      pending=null;
    }
  };
}

async function supervisePrebuilt(options) {
  const app = resolve(option(options, "--app"));
  const log = resolve(option(options, "--log"));
  const apiPort = option(options, "--api-port");
  const decoder = new StringDecoder("utf8");
  let partial = "";
  let ready = false;
  let startupFailed = false;
  let readinessPhase=0;
  const locator = options.has("--record") ? nativeLocatorSink(options) : () => {};
  const launchLease = await retainLaunchOwner();
  try {
    const { code, signal } = await runDevApp({ app, log, interactiveDesktop: true, cwd: repo, env: devRuntimeEnvironment(process.env, repo), onData: chunk => {
      locator(chunk);
      const lines = `${partial}${decoder.write(chunk)}`.split(/\r?\n/);
      partial = lines.pop() ?? "";
      for (const line of lines) {
        const next=prebuiltReadinessPhase(readinessPhase,line);
        if (next !== readinessPhase && !ready && !startupFailed && process.connected) {
          readinessPhase=next;
          process.send({type:"phase",phase:next});
        }
      }
      const failure = lines.map(startupFailureSignal).find(Boolean);
      if (!ready && failure && process.connected) {
        startupFailed = true;
        process.send({ type: "failed", error: failure });
        disconnectSupervisor();
      }
      const signal = lines.find(line => readinessSignal(line, apiPort));
      if (!ready && !startupFailed && signal) {
        ready = true;
        if (process.connected) process.send({ type: "ready", signal });
        disconnectSupervisor();
      }
    } });
    if (!ready && process.connected) {
      process.send({ type: "exited", code, signal, error: `prebuilt app exited before readiness (code=${code}, signal=${signal})` });
    }
    process.exitCode = startupFailed ? 1 : code ?? (signal ? 1 : 0);
  } catch (error) {
    if (process.connected) process.send({ type: "launch-failed", error: error.message });
    process.exitCode = 1;
  } finally { disconnectSupervisor(); launchLease?.end(); }

}

async function runPrebuilt(options) {
  const launchLease = await retainLaunchOwner();
  try {
    const locator=nativeLocatorSink(options);
    const { code, signal } = await runDevApp({
      app: resolve(option(options, "--app")), log: resolve(option(options, "--log")), interactiveDesktop: true,
      cwd: repo, env: devRuntimeEnvironment(process.env, repo), onData: chunk => { locator(chunk); process.stdout.write(chunk); },
    });
    process.exitCode = code ?? (signal ? 1 : 0);
  } finally { launchLease?.end(); }
}

async function launchPrebuilt(options) {
  const app = resolve(option(options, "--app"));
  const log = resolve(option(options, "--log"));
  const apiPort = option(options, "--api-port");
  const fixtureDeadline = process.env.CODETERM_TEST_PREBUILT_READY_DEADLINE_MS;
  const externalFrontend = process.env.DEV_FRONT_EXTERNAL === "1";
  const deadlineMs = fixtureDeadline === undefined ? 30_000 : Number(fixtureDeadline);
  if (!Number.isInteger(deadlineMs) || deadlineMs < 1 || deadlineMs > 30_000) {
    throw new Error("invalid prebuilt readiness failure deadline");
  }
  if (!existsSync(app) || !statSync(app).isFile()) throw new Error(`prebuilt app is absent: ${app}`);
  let child;
  let deadline;
  let phase=0;
  let checkingReadiness = false;
  let observedFailure = false;
  const startupWait = new AbortController();
  let cancelLaunch;
  try {
    await new Promise((resolveReady, rejectReady) => {
      const fail = (error) => rejectReady(error instanceof Error ? error : new Error(String(error)));
      cancelLaunch = () => {
        const error = new Error("prebuilt desktop launch cancelled");
        error.code = "DEV_LAUNCH_CANCELLED";
        fail(error);
      };
      process.once("SIGINT", cancelLaunch);
      process.once("SIGTERM", cancelLaunch);
      child = spawn(
        node,
        [fileURLToPath(import.meta.url), "--supervise-prebuilt", "--app", app, "--log", log, "--api-port", apiPort, ...(options.has("--record")?["--record",option(options,"--record")]:[])],
        {
          cwd: repo,
          detached: true,
          env: process.env,
          stdio: ["ignore", "ignore", "ignore", "ipc"],
          windowsHide: true,
        },
      );
      child.on("message", (message) => {
        if (message?.type === "ready" && !checkingReadiness) {
          checkingReadiness = true;
          void waitForReady({ origin: `http://127.0.0.1:${apiPort}`, signal: startupWait.signal })
            .then(() => resolveReady(message.signal), fail);
        }
        else if (message?.type === "failed") fail(new Error(message.error));
        else if (message?.type === "exited" || message?.type === "launch-failed") {
          observedFailure = true;
          fail(new Error(message.error));
        }
        else if (message?.type === "phase" && Number.isInteger(message.phase) && message.phase === phase+1 && message.phase <= 2) {
          phase=message.phase;
          clearTimeout(deadline);
          armDeadline();
        }
      });
      child.once("error", error => {
        observedFailure = true;
        fail(error);
      });
      child.once("exit", (code, signal) => {
        observedFailure = true;
        fail(new Error(`prebuilt supervisor exited before readiness (code=${code}, signal=${signal})`));
      });
      child.unref();
      console.log(`[dev-prebuilt] launched pid ${child.pid}, log ${log}`);
      function armDeadline() {
        const phaseDeadlineMs = externalFrontend && phase === 2 && fixtureDeadline === undefined ? 180_000 : deadlineMs;
        deadline = setTimeout(() => {
          const error = new Error(`prebuilt app did not report readiness within ${phaseDeadlineMs}ms (phase=${['preparation','bootstrap','attachment'][phase]})`);
          fail(error);
        }, phaseDeadlineMs);
      }
      armDeadline();
    }).then((signal) => console.log(`[dev-prebuilt] readiness signal: ${signal}`));
  } catch (error) {
    if (error.code === "DEV_LAUNCH_CANCELLED" || observedFailure || child?.exitCode != null || child?.signalCode != null) {
      await terminateOwnedChild(child);
    } else if (child?.pid) {
      if (child.connected) child.disconnect();
      error.code = "DEV_ATTACHMENT_PENDING";
    }
    throw error;
  } finally {
    startupWait.abort();
    if (cancelLaunch) {
      process.off("SIGINT", cancelLaunch);
      process.off("SIGTERM", cancelLaunch);
    }
    if (deadline) clearTimeout(deadline);
    if (child) child.removeAllListeners();
  }
}

function runRegularPrep() {
  const startServer = !process.argv.includes("--no-server");
  run(cargo, ["build", "--release", "--manifest-path", "cli/Cargo.toml"]);
  process.env.CODETERM_DEV = "1";
  run(node, ["scripts/bundle-cli.js"]);
  if (startServer) run(node, ["scripts/dev-static-server.mjs"]);
}

const command = process.argv[2];
try {
  if (command === "--check-attachment") checkAttachment(parseOptions(process.argv.slice(3)));
  else if (command === "--write-provenance") writeProvenance(parseOptions(process.argv.slice(3)));
  else if (command === "--check-provenance") checkProvenance(parseOptions(process.argv.slice(3)));
  else if (command === "--check-server-reload") checkProvenance(parseOptions(process.argv.slice(3)), true);
  else if (command === "--check-runtime") checkRuntime(parseOptions(process.argv.slice(3)));
  else if (command === "--supervise-prebuilt") await supervisePrebuilt(parseOptions(process.argv.slice(3)));
  else if (command === "--run-prebuilt") await runPrebuilt(parseOptions(process.argv.slice(3)));
  else if (command === "--launch-prebuilt") await launchPrebuilt(parseOptions(process.argv.slice(3)));
  else runRegularPrep();
} catch (error) {
  console.error(`[dev-prep] ${error.message}`);
  process.exit(error.code === "DEV_ATTACHMENT_PENDING" ? 3 : 1);
}
