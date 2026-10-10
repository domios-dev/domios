import { spawn, execFileSync } from "node:child_process";
import { writeFileSync, renameSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { launchRestart } from "./restart.mjs";

const directory = dirname(fileURLToPath(import.meta.url));
const root = resolve(directory, "../..");
const [out, round] = process.argv.slice(2);
const port = Number(process.env.CODETERM_API_PORT);
const instance = process.env.CODETERM_INSTANCE || "";
if (!((port === 7686 && !instance) || (port === 7697 && instance === "aiderqa"))) {
  throw new Error("Run the launcher through the selected DEV wrapper");
}
const path = join(out, `restart-${round}.json`);
const record = (evidence) => {
  writeFileSync(`${path}.tmp`, JSON.stringify({ instance, port, ...evidence }, null, 2) + "\n");
  renameSync(`${path}.tmp`, path);
};
const snapshot = () => JSON.parse(execFileSync("python3", [join(directory, "listener.py"), String(port)], { encoding: "utf8", timeout: 30_000 }));
const command = (...args) => new Promise((accept, reject) => {
  const child = spawn(process.env.ENDURE_BASH || "bash", [join(root, "build.sh"), ...args], {
    cwd: root, stdio: "inherit", signal: AbortSignal.timeout(600_000),
  });
  child.on("error", reject);
  child.on("close", (code) => code === 0 ? accept() : reject(new Error(`build.sh ${args[0]} exit ${code}`)));
});
const evidence = await launchRestart({ snapshot, record,
  stop: () => command("kill"), start: () => command("dev-prebuilt", "--wait", "--wait-build"),
});
console.log(JSON.stringify(evidence));
process.exitCode = evidence.pass ? 0 : 1;
