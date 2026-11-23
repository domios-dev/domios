import type { CSSProperties, ReactNode } from "react";
import { MessageBubble } from "@codeterm/chat-ui";
import type { ChatOrigin } from "@codeterm/chat-core";
import { TabItem } from "../../../../../src/components/Sidebar/TabItem";
import type { Tab } from "../../../../../src/lib/types";
import { ProductProviders } from "../../../gallery/fixtures";
import { StatusRow } from "../../fixtures/status";
import { useStage } from "../../engine";

export const noop = () => undefined;
export const spring = (t: number, at: number, speed = 1) => {
  const u = Math.max(0, t - at) * speed;
  return 1 - Math.exp(-4.6 * u) * (Math.cos(10 * u) + 0.46 * Math.sin(10 * u));
};
export const kick = (t: number, at: number, speed = 1) => {
  const u = Math.max(0, t - at) * speed;
  return Math.exp(-3.2 * u) * Math.sin(12 * u);
};

export function Toy({ x, y, scale = 1, turn = 0, squash = 0, children, style }: {
  x: number; y: number; scale?: number; turn?: number; squash?: number; children: ReactNode; style?: CSSProperties;
}) {
  return <div style={{ position: "absolute", left: x, top: y, transform: `rotate(${turn}deg) scale(${scale * (1 + squash)}, ${scale / (1 + squash)})`, transformOrigin: "0 0", ...style }}>{children}</div>;
}

export function AgentToy({ tab, width = 380 }: { tab: Tab; width?: number }) {
  return <ProductProviders tabs={[tab]}>
    <div style={{ width, background: "var(--ct-sidebar-bg)", border: "1px solid var(--ct-border)", borderRadius: 12, padding: 12, boxShadow: "var(--ct-shadow-lg)" }}>
      <div className="gallery-sidebar"><TabItem tab={tab} index={0} isActive onClick={noop} onClose={noop} canClose workspaceId={tab.workspace_id ?? "gallery-workspace"} hasAgent status={tab.agent_state?.state === "working" ? "Working" : undefined} /></div>
      <div style={{ paddingTop: 10 }}><StatusRow tab={tab} /></div>
    </div>
  </ProductProviders>;
}

export function Bubble({ id, text, origin, assistant = false, width = 390 }: {
  id: string; text: string; origin?: ChatOrigin; assistant?: boolean; width?: number;
}) {
  return <div className="gallery-transcript" style={{ width }}>
    <MessageBubble message={{ id, type: assistant ? "assistant" : "user", content: text, ...(origin ? { inboundKind: "system" as const } : {}) }} origin={origin} onOriginClick={noop} onOpenJobs={noop} onStopReminder={noop} />
  </div>;
}

export function Set({ title, children, style }: {
  title: ReactNode; children: ReactNode; style?: CSSProperties;
}) {
  const { portrait } = useStage();
  return <div style={{ position: "absolute", inset: 0, background: "var(--ct-bg)", color: "var(--ct-fg)", ...style }}>
    <div style={{ position: "absolute", left: portrait ? 120 : 80, top: portrait ? 54 : 48, width: portrait ? 850 : 1720 }}>
      <div style={{ fontSize: 72, lineHeight: 1.02, letterSpacing: "-0.055em", fontWeight: 750 }}>{title}</div>
    </div>
    {children}
  </div>;
}
