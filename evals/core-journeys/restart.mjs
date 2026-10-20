import { spawn } from "node:child_process";
import { renameSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { launchRestart } from "../restart-endurance/restart.mjs";
import { root, target } from "./lib/env.mjs";
import { listenerIdentity } from "./lib/listener.mjs";

export function parseRestartArgs(argv) {
  const options = { out: null, stopIn: root, startIn: root, start: "dev-prebuilt", label: "restart" };
  for (let index = 0; index < argv.length; index += 2) {
    const [flag, value] = [argv[index], argv[index + 1]];
    if (value === undefined) throw new Error(`missing value for ${flag}`);
    if (flag === "--out") options.out = resolve(value);
    else if (flag === "--stop-in") options.stopIn = resolve(value);
    else if (flag === "--start-in") options.startIn = resolve(value);
    else if (flag === "--start") options.start = value;
    else if (flag === "--label") options.label = value;
    else throw new Error(`unknown restart flag ${flag}`);
  }
  if (!options.out) throw new Error("--out DIR is required");
  if (!["dev-prebuilt", "dev"].includes(options.start)) throw new Error("--start must be dev-prebuilt or dev");
  return options;
}

function build(cwd, args) {
  return new Promise((accept, reject) => {
    const child = spawn("bash", [join(cwd, "build.sh"), ...args], { cwd, stdio: "inherit", signal: AbortSignal.timeout(1_800_000) });
    child.on("error", reject);
    child.on("close", (code) => (code === 0 ? accept() : reject(new Error(`build.sh ${args.join(" ")} in ${cwd} exit ${code}`))));
  });
}

async function main() {
  const options = parseRestartArgs(process.argv.slice(2));
  const dev = target();
  const path = join(options.out, `${options.label}.json`);
  const record = (evidence) => {
    writeFileSync(`${path}.tmp`, `${JSON.stringify({ instance: dev.instance, port: dev.port, ...options, ...evidence }, null, 2)}\n`);
    renameSync(`${path}.tmp`, path);
  };
  const evidence = await launchRestart({
    record, snapshot: () => listenerIdentity(dev.port),
    stop: () => build(options.stopIn, ["kill"]),
    start: () => build(options.startIn, [options.start, "--wait", "--wait-build"]),
  });
  console.log(`RESTART ${evidence.pass ? "PASS" : "FAIL"} ${JSON.stringify({ before: evidence.before, after: evidence.after, reason: evidence.reason })}`);
  return evidence.pass ? 0 : 1;
}

if (process.argv[1] && resolve(process.argv[1]).endsWith(join("core-journeys", "restart.mjs"))) {
  main().then((code) => { process.exitCode = code; }, (error) => { console.error(error.stack); process.exitCode = 2; });
}
