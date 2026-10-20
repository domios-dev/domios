// Samples the DEV page CPU for a few seconds and prints self time and app-frame totals.
//   node scripts/dev-perf/profile.mjs [--seconds 10] [--port 9223]
import { connectPage, sleep } from "./cdp.mjs";

const args = process.argv.slice(2);
const option = (name) => { const index = args.indexOf(name); return index >= 0 ? args[index + 1] : undefined; };
const seconds = Number(option("--seconds") ?? 10);
const page = await connectPage(option("--port") ? Number(option("--port")) : undefined);

await page.call("Profiler.enable");
await page.call("Profiler.setSamplingInterval", { interval: 200 });
await page.call("Profiler.start");
await sleep(seconds * 1000);
const { profile } = await page.call("Profiler.stop");
page.close();

const nodes = new Map(profile.nodes.map((node) => [node.id, node]));
const parents = new Map();
for (const node of profile.nodes) for (const child of node.children ?? []) parents.set(child, node.id);
const frameName = (node) => {
  const url = node.callFrame.url.split("?")[0].split("/").slice(-2).join("/");
  return `${node.callFrame.functionName || "(anonymous)"} ${url}:${node.callFrame.lineNumber + 1}`;
};
const self = new Map();
profile.samples.forEach((id, index) => self.set(id, (self.get(id) ?? 0) + (profile.timeDeltas[index] ?? 0)));
const selfByName = new Map();
const totalByName = new Map();
for (const [id, micros] of self) {
  selfByName.set(frameName(nodes.get(id)), (selfByName.get(frameName(nodes.get(id))) ?? 0) + micros);
  const seen = new Set();
  for (let current = id; current !== undefined; current = parents.get(current)) {
    const name = frameName(nodes.get(current));
    if (seen.has(name)) continue;
    seen.add(name);
    totalByName.set(name, (totalByName.get(name) ?? 0) + micros);
  }
}
const top = (map, count, keep = () => true) => [...map].filter(([name]) => keep(name)).sort((a, b) => b[1] - a[1]).slice(0, count)
  .map(([name, micros]) => `${(micros / 1000).toFixed(0).padStart(7)}ms ${name}`);
console.log("self time:\n" + top(selfByName, 18).join("\n"));
console.log("\napp frames, total time:\n" + top(totalByName, 25, (name) => /\.tsx?:\d+$/.test(name) && !name.includes("deps/")).join("\n"));
