import { client } from "./lib/env.mjs";
import { requireCdp } from "./lib/setup.mjs";
import { loadAreas, runs, scanSelectionFlags, selectionFromFlags } from "./lib/selection.mjs";

try {
  const { api } = client();
  const selection = selectionFromFlags(loadAreas(), scanSelectionFlags(process.argv.slice(2)));
  const health = await api("/api/health");
  if (["J1", "J2", "J3", "J4"].some(id => runs(selection, id))) requireCdp(health);
} catch (error) {
  console.error(`FAIL prerequisite: ${error.message}`);
  process.exitCode = 7;
}
