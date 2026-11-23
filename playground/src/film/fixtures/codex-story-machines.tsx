import { MessageBubble } from "@codeterm/chat-ui";
import type { ChatOrigin } from "@codeterm/chat-core";
import type { CSSProperties, ReactNode } from "react";
import { DetailContent } from "../../../../src/components/Canvas/canvasRender/DetailContent";
import { PLAN, PRODUCT_TASK } from "./plan";
import { useStage, range } from "../engine";

export const CREW = ["General", "Orchestrator", "Worker"] as const;
export type Crew = typeof CREW[number];
const noop = () => undefined;

export function Frame({ title, children }: { title: ReactNode; children: ReactNode }) {
  const { portrait } = useStage();
  return <div style={{ position: "absolute", inset: 0, color: "var(--ct-fg)", background: "var(--ct-bg)" }}>
    <div style={{ position: "absolute", left: portrait ? 60 : 88, top: portrait ? 100 : 130,
      width: portrait ? 850 : 630, fontSize: portrait ? 80 : 88, fontWeight: 550, letterSpacing: "-0.055em", lineHeight: 1.04 }}>{title}</div>
    {children}
  </div>;
}

export function Stack({ children, rows = 1 }: { children: ReactNode; rows?: 1 | 2 | 3 }) {
  const { portrait } = useStage();
  return <div className="gallery-transcript" style={{ position: "absolute", left: portrait ? 54 : 790,
    top: portrait ? 350 : rows === 3 ? 60 : rows === 2 ? 120 : 210,
    width: portrait ? 425 : 540, transform: portrait ? "scale(2.05)" : "scale(1.82)",
    transformOrigin: "top left", display: "flex", flexDirection: "column", gap: 12 }}>{children}</div>;
}

export function Chat({ speaker, text, t, at = -1 }: {
  speaker: Crew; text: string; t: number; at?: number;
}) {
  const origin: ChatOrigin = { kind: "tab", title: speaker, tabIdShort: "", model: speaker === "Worker" ? "codex" : speaker === "Orchestrator" ? "opus" : undefined, report: false };
  const k = range(t, at, at + 0.35);
  return <div style={{ visibility: t >= at ? "visible" : "hidden", transform: "translateY(" + ((1 - k) * 7) + "px)" }}>
    <MessageBubble message={{ id: "machines-" + speaker + text, type: "user", inboundKind: "system", content: text }}
      fontScale={2} origin={origin} onOriginClick={noop} onOpenJobs={noop} onStopReminder={noop} />
  </div>;
}

export function UpdateReceipt({ t }: { t: number }) {
  const { portrait } = useStage();
  const rows = ["Safe points confirmed", "Sessions saved", "Cold update", "Resume exact sessions"];
  const active = Math.min(3, Math.floor(t / 0.9));
  return <div style={{ position: "absolute", left: portrait ? 60 : 790, top: portrait ? 400 : 140,
    width: portrait ? 850 : 990, borderTop: "2px solid var(--ct-border)" }}>
    {rows.map((row, i) => <div key={row} style={{ display: "flex", alignItems: "center", gap: 24,
      padding: portrait ? "44px 0" : "48px 0", borderBottom: "2px solid var(--ct-border)", fontSize: portrait ? 44 : 48,
      color: i <= active ? "var(--ct-fg)" : "var(--ct-muted)" }}>
      <span style={{ color: i <= active ? "var(--ct-green)" : "var(--ct-muted)", width: 40 }}>{i <= active ? "✓" : "·"}</span>{row}
    </div>)}
  </div>;
}

export function CoolingTask() {
  const task = { ...PRODUCT_TASK, id: "machines-discovery", key: "GC-1", name: "Sensor discovery",
    body: "Discover sensor interfaces before fan control. Live hardware writes require the owner's OK.",
    acceptance_criteria: [], milestones: [], section_ids: [], assigned_tab_id: null, lane: undefined,
    executor_role: "Cooling executor", role_guidance: "", status: "in_progress" as const };
  const style: CSSProperties = { padding: 20, border: "1px solid var(--ct-border)", borderRadius: 12, background: "var(--ct-surface)" };
  return <div style={style}><DetailContent kind="task" plan={PLAN} task={task} tabs={{}} /></div>;
}
