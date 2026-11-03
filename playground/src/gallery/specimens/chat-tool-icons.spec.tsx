import { useState, type CSSProperties } from "react";
import type { ToolEvent } from "@codeterm/chat-core";
import { CodetermActionCard } from "../../../../packages/chat-ui/src/toolcard/CodetermActionCard";
import { ToolActivityStream } from "../../../../packages/chat-ui/src/toolcard/ToolActivityStream";
import { DEFAULT_THEME, themeToCSS } from "../../../../src/lib/themeCss";
import { defineSpecimen } from "../types";

const commands = ["domios tab close tab_20117ce6", "domios agent report --status in_review", "domios agent ack", "domios plan show pl_example", "domios send --tab target hello", "domios notify send Ready", "domios mem get mem_example", "echo hello"];
const members: ToolEvent[] = commands.map((command, index) => ({
  id: `tool-${index}`, phase: "result", kind: "command", status: "success", summary: command,
  message: { id: `tool-${index}`, type: "tool_call", content: "", toolInput: { command } },
}));
for (const kind of ["read", "edit", "search"] as const) members.push({ ...members[0], id: kind, kind, summary: `${kind} source file` });
for (const status of ["pending", "error", "interrupted", "unresolved"] as const) members.push({ ...members[0], id: status, status, summary: `${status}: domios tab close tab_20117ce6` });
const stream: ToolEvent = { ...members[0], id: "stream", summary: "Tool kinds and outcomes", stream: { flowId: "icons", active: false, members, entries: members.map(event => ({ kind: "action", event })) } };

function ToolIconStage({ light }: { light: boolean }) {
  const [expanded, setExpanded] = useState(true);
  const theme = light ? { ...DEFAULT_THEME, background: "#ffffff", foreground: "#24292f", bright_black: "#57606a", blue: "#0969da", green: "#1a7f37", yellow: "#9a6700", red: "#cf222e" } : DEFAULT_THEME;
  return <div style={{ ...themeToCSS(theme), color: "var(--ct-fg)", background: "var(--ct-bg)", padding: 20 } as CSSProperties}>
    <ToolActivityStream event={stream} expanded={expanded} onToggle={setExpanded} renderAction={(event, open, toggle) => <CodetermActionCard event={event} expanded={open} onToggle={() => toggle(!open)} />} />
    {members.slice(0, 2).map(event => <CodetermActionCard key={event.id} event={event} expanded={false} onToggle={() => {}} />)}
  </div>;
}

export default defineSpecimen({
  id: "chat-tool-icons", title: "Chat tool icons", category: "Chat", stage: "wide",
  where: "packages/chat-ui/src/toolcard/toolVisualModel.ts", Component: ToolIconStage,
  states: [{ id: "dark", label: "Dark · kinds and outcomes", props: { light: false } }, { id: "light", label: "Light · kinds and outcomes", props: { light: true } }],
});
