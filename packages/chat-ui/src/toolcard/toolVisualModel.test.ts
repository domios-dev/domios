import assert from "node:assert/strict";
import { Bell, Bot, BookOpen, Check, CircleX, FileEdit, FileSearch, Grid3X3, LogOut, Map, MessageSquareText, Search, Send, Terminal, TriangleAlert, Wrench } from "lucide-react";
import type { ToolEvent } from "@codeterm/chat-core";
import { actionVisual, toolEventVisual, toolOutcomeVisual, toolStepVisual } from "./toolVisualModel";
import { toolTones } from "./toolKindVisual";
import { recognize } from "../codeterm/recognize";

function event(command: string, kind: ToolEvent["kind"] = "command", status: ToolEvent["status"] = "success"): ToolEvent {
  return { id: "call", kind, status, phase: "call", summary: command, message: { id: "call", type: "tool_call", content: "", toolInput: { command } } };
}

for (const [command, glyph] of [
  ["tab close tab_20117ce6", LogOut], ["tab inspect tab_20117ce6", Grid3X3],
  ["agent report --status in_review", MessageSquareText], ["agent ack", Bot],
  ["plan show pl_example", Map], ["send --tab target hello", Send],
  ["notify send title", Bell], ["mem get mem_example", BookOpen],
] as const) {
  for (const cli of ["domios", "codeterm"]) {
    const invocation = recognize(`${cli} ${command}`);
    assert.ok(invocation, command);
    assert.equal(actionVisual(invocation).glyph, glyph, command);
    const visual = toolEventVisual(event(`${cli} ${command}`));
    assert.equal(visual.glyph, glyph, command);
    assert.notEqual(visual.glyph, CircleX, command);
    assert.notEqual(visual.color, toolTones.danger, command);
  }
}

for (const [kind, glyph] of [["command", Terminal], ["edit", FileEdit], ["read", FileSearch], ["search", Search], ["generic", Wrench]] as const) {
  assert.equal(toolEventVisual(event("echo hello", kind)).glyph, glyph);
}
assert.equal(toolEventVisual(event("rg pattern src")).glyph, Search);
assert.equal(toolEventVisual(event("cat source.ts")).glyph, FileSearch);
assert.equal(toolOutcomeVisual.success.glyph, Check);
assert.equal(toolOutcomeVisual.error.glyph, CircleX);
assert.equal(toolOutcomeVisual.interrupted.glyph, TriangleAlert);
assert.equal(toolOutcomeVisual.unresolved.label, "no result");
for (const [status, tone] of [["success", "success"], ["pending", "info"], ["error", "danger"], ["interrupted", "warning"], ["unresolved", "muted"]] as const) {
  assert.equal(toolOutcomeVisual[status].color, toolTones[tone]);
  const visual = toolEventVisual(event("domios tab close tab_20117ce6", "command", status));
  assert.equal(visual.glyph, LogOut);
  assert.equal(visual.color, toolTones[status === "success" ? "info" : tone]);
  const step = toolStepVisual(event("domios tab close tab_20117ce6", "command", status));
  assert.equal(step.glyph, status === "success" ? LogOut : toolOutcomeVisual[status].glyph);
  assert.equal(step.label, `close tab, ${toolOutcomeVisual[status].label}`);
}
console.log("toolVisualModel: action families, shell/file/search kinds and all five outcomes passed");

