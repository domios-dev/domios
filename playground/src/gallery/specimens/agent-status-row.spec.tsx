import { AgentStatusRow, type AgentStatusRowProps } from "../../ds/AgentStatusRow";
import { defineSpecimen } from "../types";

const base = { providerName: "Claude Code", glyph: "✦", glyphColor: "var(--ct-orange)", model: "fable", effort: "high" };

function AgentStatusRowSpecimen(props: AgentStatusRowProps) {
  return <div className="gallery-specimen-fill"><AgentStatusRow {...props} /></div>;
}

export default defineSpecimen({
  id: "agent-status-row",
  title: "Agent status row",
  stage: "wide",
  category: "Status",
  where: "src/components/Terminal/AgentStateIndicator.tsx",
  notes: "The provider glyph lives inside the model capsule, the dots follow the capsule, the phase text follows the dots with no separator, and Stop is a pill at the far end. At rest the dots stay in place, grey and still, and the Stop slot keeps its width.",
  states: [
    { id: "working", label: "Working", props: { ...base, phase: "work", status: "Planning gallery catalog… (9m 23s · ↓ 35.3k tokens)" } },
    { id: "thinking", label: "Thinking", props: { ...base, phase: "thinking", status: "Considering the next move…" } },
    { id: "streaming", label: "Streaming", props: { ...base, phase: "streaming", status: "Writing response…" } },
    { id: "idle", label: "Idle · dots at rest", props: { ...base, phase: "idle", status: "Churned for 1m 37s" } },
    { id: "static", label: "Not switchable", props: { ...base, effort: undefined, switchable: false, phase: "work", status: "Working" } },
  ],
  Component: AgentStatusRowSpecimen,
});
