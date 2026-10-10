// Sample the renderer's main thread for a fixed window and print the busy share
// plus the top functions by self time. The window length is the measurement itself.
// Usage: node cpu-profile.mjs --cdp 9263 --vite 1460 [--ms 20000] [--top 25] [--out profile.cpuprofile]
import { writeFileSync } from "node:fs";
import { connect, pageTarget } from "./cdp.mjs";

const argv = process.argv.slice(2);
const arg = (name, fallback) => (argv.includes(`--${name}`) ? argv[argv.indexOf(`--${name}`) + 1] : fallback);
const windowMs = Number(arg("ms", 20000));
const top = Number(arg("top", 25));

const target = await pageTarget(Number(arg("cdp")), Number(arg("vite")));
const client = await connect(target.webSocketDebuggerUrl);
let profile;
try {
  await client.send("Profiler.enable");
  await client.send("Profiler.setSamplingInterval", { interval: 1000 });
  await client.send("Profiler.start");
  await new Promise((resolve) => setTimeout(resolve, windowMs));
  ({ profile } = await client.send("Profiler.stop"));
} finally {
  client.close();
}
if (arg("out")) writeFileSync(arg("out"), JSON.stringify(profile));

const byId = new Map(profile.nodes.map((node) => [node.id, node]));
const deltas = profile.timeDeltas;
const selfUs = new Map();
let totalUs = 0;
let idleUs = 0;
for (let index = 0; index < profile.samples.length; index += 1) {
  const node = byId.get(profile.samples[index]);
  const us = deltas[index] ?? 0;
  totalUs += us;
  const { functionName, url, lineNumber } = node.callFrame;
  if (functionName === "(idle)") idleUs += us;
  const file = url ? url.split("/").pop() : "";
  const key = `${functionName || "(anonymous)"} ${file}:${lineNumber + 1}`;
  selfUs.set(key, (selfUs.get(key) ?? 0) + us);
}
const ms = (us) => (us / 1000).toFixed(0);
console.log(`window ${ms(totalUs)} ms, idle ${ms(idleUs)} ms, busy ${(((totalUs - idleUs) / totalUs) * 100).toFixed(1)}%`);
for (const [key, us] of [...selfUs].sort((a, b) => b[1] - a[1]).slice(0, top)) {
  console.log(`${ms(us).padStart(7)} ms ${((us / totalUs) * 100).toFixed(1).padStart(5)}%  ${key.slice(0, 110)}`);
}
