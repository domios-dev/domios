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

for (const [kind, source, state, summary, fact] of [
  ["github", { repo: "owner/project", resource: "run", id: 42 }, "failing", "failing · Linux, Windows", "owner/project · run 42"],
  ["github", { org: "owner", resource: "runners", label: "linux-build" }, "offline", "offline · 0 online", "owner · runners · linux-build"],
  ["github", { repo: "owner/project", resource: "runners", label: "linux-build" }, "online", "online · 2 online", "owner/project · runners · linux-build"],
  ["http", { url: "https://example.com/health", expected_status: "204" }, "down", "down · 503", "https://example.com/health"],
  ["http", { url: "https://example.com/health", expected_status: "2xx" }, "up", "up · 200", "https://example.com/health"],
] as const) {
  const customized = pipelineModel({ id: "edited", name: "Exported and renamed", state:"idle", run_id:"run", current_step:null, last_event_at_ms:1000,
    steps:[{ index:0, name:"My customized step", title:"My customized step", kind, source, source_state:state, state:"done", outcome:state, started_at_ms:900, ended_at_ms:1000, result_summary:summary, reason:null }] });
  window.document.body.innerHTML = renderToStaticMarkup(<BlueprintPipelineCanvas pipeline={customized} />);
  const card = [...window.document.querySelectorAll("article")].find((node) => node.textContent.includes(kind === "github" ? "GitHub source" : "HTTP health"));
  assert.ok(card);
  assert.ok(card.textContent.includes(fact));
  assert.ok(card.textContent.includes(state));
  assert.ok(card.textContent.includes(summary));
  assert.equal(card.querySelector("svg title")?.textContent, kind === "github" ? "GitHub" : undefined);
}
