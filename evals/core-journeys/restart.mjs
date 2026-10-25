import { spawn } from "node:child_process";
import { renameSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { launchRestart } from "../restart-endurance/restart.mjs";
import { client, root, target } from "./lib/env.mjs";
import { listenerIdentity } from "./lib/listener.mjs";
import { readBuildIdentity } from "./lib/build-identity.mjs";

export function parseRestartArgs(argv) {
  const options = { out: null };
  for (let index = 0; index < argv.length; index += 2) {
    const [flag, value] = [argv[index], argv[index + 1]];
    if (value === undefined) throw new Error(`missing value for ${flag}`);
    if (flag === "--out") options.out = resolve(value);
    else throw new Error(`unknown restart flag ${flag}`);
  }
  if (!options.out) throw new Error("--out DIR is required");
  return options;
}

function productEnvironmentKeys(identity) {
  const prefix = identity.cli.instance.replace(/_INSTANCE$/, "");
  return ["INSTANCE", "API_PORT", "API_URL", "API_TOKEN", "DATA_DIR", "SOCKET_PATH", "VITE_PORT", "CDP_PORT", "DEV_INSTANCE",
    "DEV_API_PORT", "DEV_VITE_PORT", "DEV_CDP_PORT", "DEV_DATA_DIR", "DEV_IDENTIFIER", "CLI_PATH", "TAB_ID", "PARENT_TAB_ID",
    "SESSION_ID", "SESSION_BINDING_NONCE", "TEAM_ID", "PLAN_ID", "TASK_ID", "ATTEMPT_ID", "STARTUP_ATTEMPT_ID"]
    .map((suffix) => `${prefix}_${suffix}`);
}

export function restartEnvironment(identity, dev, launch, inherited = process.env) {
  if (launch?.name !== dev.instance || Number(launch.api_port) !== dev.port) {
    throw new Error("Restart launch identity differs from the isolated bench target");
  }
  const port = value => {
    if (!/^[0-9]+$/.test(String(value)) || Number(value) < 1 || Number(value) > 65535) {
      throw new Error("Restart requires the original DEV frontend and CDP ports");
    }
    return String(value);
  };
  const env = { ...inherited };
  for (const key of productEnvironmentKeys(identity)) delete env[key];
  env[identity.request.instance] = dev.instance;
  env[identity.request.port] = String(dev.port);
  env[identity.development.instance] = dev.instance;
  env[identity.development.port] = String(dev.port);
  const devPrefix = identity.development.instance.replace(/_INSTANCE$/, "");
  env[`${devPrefix}_VITE_PORT`] = port(launch.vite_port);
  env[`${devPrefix}_CDP_PORT`] = port(launch.cdp_port);
  return env;
}

function build(args, env) {
  return new Promise((accept, reject) => {
    const child = spawn("bash", [join(root, "build.sh"), ...args], { cwd: root, env, stdio: "inherit", signal: AbortSignal.timeout(1_800_000) });
    child.on("error", reject);
    child.on("close", (code) => (code === 0 ? accept() : reject(new Error(`build.sh ${args.join(" ")} in ${root} exit ${code}`))));
  });
}

async function main() {
  const options = parseRestartArgs(process.argv.slice(2));
  const dev = target();
  const health = await client(dev).api("/api/health");
  const env = restartEnvironment(readBuildIdentity(root), dev, health.instance);
  const path = join(options.out, "restart.json");
  const record = (evidence) => {
    writeFileSync(`${path}.tmp`, `${JSON.stringify({ instance: dev.instance, port: dev.port, ...options, ...evidence }, null, 2)}\n`);
    renameSync(`${path}.tmp`, path);
  };
  const evidence = await launchRestart({
    record, snapshot: () => listenerIdentity(dev.port),
    stop: () => build(["kill"], env),
    start: () => build(["dev-prebuilt", "--wait", "--wait-build"], env),
  });
  console.log(`RESTART ${evidence.pass ? "PASS" : "FAIL"} ${JSON.stringify({ before: evidence.before, after: evidence.after, reason: evidence.reason })}`);
  return evidence.pass ? 0 : 1;
}

if (process.argv[1] && resolve(process.argv[1]).endsWith(join("core-journeys", "restart.mjs"))) {
  main().then((code) => { process.exitCode = code; }, (error) => { console.error(error.stack); process.exitCode = 2; });
}
