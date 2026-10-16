import fs from "node:fs";
import path from "node:path";
import { dcJson } from "../core-journeys/lib/env.mjs";

const [mode, output] = process.argv.slice(2);
if (!["setup", "cleanup"].includes(mode) || !output) throw new Error("Usage: node evals/general-overview/demo.mjs setup|cleanup RECEIPT.json");
const receipt = path.resolve(output);
fs.mkdirSync(path.dirname(receipt), { recursive: true });
let state;
const save = () => fs.writeFileSync(receipt, JSON.stringify(state, null, 2));
if (mode === "cleanup") {
  state = JSON.parse(fs.readFileSync(receipt, "utf8"));
  if (state.kind !== "general-overview-demo" || state.port !== 7686) throw new Error("Not an owned DEV demo receipt");
  for (const id of state.crons) await dcJson(["cron", "delete", id]);
  for (const id of state.services) await dcJson(["service", "delete", id]);
  if (state.workspace) await dcJson(["workspace", "close", state.workspace]);
  state.cleaned = true;
  save();
} else {
  if (fs.existsSync(receipt)) throw new Error("Receipt exists; clean the recorded demo before creating another");
  state = { kind: "general-overview-demo", port: 7686, crons: [], services: [], tabs: [] };
  save();
  const created = await dcJson(["workspace", "create", "OverviewDemo", "--provision"]);
  const workspace = created.workspace ?? created;
  state.workspace = workspace.id;
  save();
  for (const [title, purpose] of [["Demoqa", "qa"], ["Demologs", "logs"]]) {
    const tab = await dcJson(["tab", "new", "--workspace", workspace.id, "--no-team", "--title", title, "--purpose", purpose, "--description", "DEV overview demonstration; idle shell, no provider launched"]);
    state.tabs.push(tab.tab_id ?? tab.id);
    save();
  }
  for (const [name, port] of [["Demo API healthy", "7686"], ["Demo worker offline", "9"]]) {
    const service = await dcJson(["service", "add", name, "--health-mode", "tcp", "--host", "127.0.0.1", "--port", port, "--health-interval", "300", "--description", "DEV-only overview demonstration; observes a port and launches nothing"]);
    const id = service.service?.id ?? service.id;
    if (!id) throw new Error("Service response has no id");
    state.services.push(id);
    save();
  }
  for (const name of ["Demo backup paused", "Demo cleanup paused"]) {
    const cron = await dcJson(["cron", "create", name, "--workspace", workspace.id, "--no-team", "--schedule", "0 0 1 1 *", "--task", "DEV display fixture only; do not execute", "--description", "Paused DEV overview demonstration"]);
    const id = cron.cron?.id ?? cron.job?.id ?? cron.id;
    if (!id) throw new Error("Cron response has no id");
    state.crons.push(id);
    save();
    await dcJson(["cron", "disable", id]);
  }
  state.ready = true;
  save();
}
console.log(JSON.stringify({ receipt, ...state }));
