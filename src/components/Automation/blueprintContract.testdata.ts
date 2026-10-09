import { readFileSync } from "node:fs";
import type { AutomationGraph } from "./types";

export function pipelineContractGolden(): AutomationGraph {
  return JSON.parse(readFileSync(new URL("../../../tests/fixtures/blueprint-pipeline.golden.json", import.meta.url), "utf8")) as AutomationGraph;
}
