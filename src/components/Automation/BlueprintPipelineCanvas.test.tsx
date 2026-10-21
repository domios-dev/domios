import assert from "node:assert/strict";
import { renderToStaticMarkup } from "react-dom/server";
import { Window } from "happy-dom";
import { BlueprintPipelineCanvas } from "./BlueprintPipelineCanvas";
import { pipelineModel } from "./pipelineModel";

const pipeline = pipelineModel({
  id: "ci", name: "CI watch", state: "idle", run_id: "run", current_step: null, last_event_at_ms: 1000,
  steps: [{ index: 0, name: "Check CI", title: "GitHub check", kind: "github",
    source: { repo: "cli/cli", resource: "run", id: 39652641013 },
    state: "done", outcome: "done", started_at_ms: 900, ended_at_ms: 1000,
    result_summary: "query completed", reason: null }],
});
const window = new Window();
window.document.body.innerHTML = renderToStaticMarkup(<BlueprintPipelineCanvas pipeline={pipeline} />);
const card = [...window.document.querySelectorAll("article")].find((node) => node.textContent.includes("GitHub source"));
assert.ok(card, "GitHub source has a card");
assert.equal(card.querySelector("svg title")?.textContent, "GitHub");
assert.ok(card.textContent.includes("cli/cli · run 39652641013"));
assert.ok(card.textContent.includes("done"));
assert.ok(card.textContent.includes("query completed"));
assert.ok(card.querySelector("time")?.getAttribute("dateTime"));
console.log("GitHub pipeline card content passed");
