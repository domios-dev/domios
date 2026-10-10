import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const targets = JSON.parse(readFileSync(new URL("./targets.json", import.meta.url), "utf8"));
export function assertTarget(instance, port) {
  const target = targets.find((target) => target.instance === instance && target.port === Number(port));
  if (!target) throw new Error(`Refused endurance target ${instance || "default"}:${port}; use aiderqa:7697 or endureqa:7699`);
  return { ...target };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  assertTarget(process.argv[2], process.argv[3]);
}
