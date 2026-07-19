import { useState, type ReactNode } from "react";
import { ChevronRight, Plus, Terminal, MessageSquare } from "lucide-react";
import { ChatSurface } from "./ChatSurface";
import "../../../../src/styles/agent-state.css";
import "./motion-system.css";

const WORKSPACES = ["CT", "BN", "RK", "EQ"];

function Section({ title, count, defaultOpen, children }: { title: string; count?: string; defaultOpen?: boolean; children: ReactNode }) {
  const [open, setOpen] = useState(!!defaultOpen);
  return (
    <div>
      <button type="button" className="msx-disclosure-btn msx-side-label" aria-expanded={open} onClick={() => setOpen((v) => !v)}>
        <ChevronRight className="msx-side-chevron" aria-hidden />
        {title}
        {count && <span style={{ marginLeft: "auto", color: "#5b6672", letterSpacing: 0 }}>{count}</span>}
      </button>
      <div className="msx-reveal" data-open={open}><div>{children}</div></div>
    </div>
  );
}

// The genuine phase-dots, corner-sized, for workspace activity.
function CardWork({ phase = "work" }: { phase?: string }) {
  return (
    <span className="msx-ws-work ct-agent-row" data-agent-state="working" data-agent-phase={phase} aria-hidden>
      <span className="ct-phase-dots"><i /><i /><i /></span>
    </span>
  );
}

export function MotionSystemConcept() {
  const [selectedWs, setSelectedWs] = useState(0);
  const [selectedRow, setSelectedRow] = useState(1);

  return (
    <div className="msx">
      <div className="msx-stage">
        {/* ── Sidebar ─────────────────────────────────────────── */}
        <div>
          <div className="msx-col-head"><span>Sidebar</span><span>disclosure · activity</span></div>
          <div className="msx-panel msx-side">
            <Section title="General" defaultOpen>
              <div style={{ display: "grid", gap: 1, padding: "2px 0 6px" }}>
                <button type="button" className="msx-row" data-selected={selectedRow === 0} onClick={() => setSelectedRow(0)}>
                  <MessageSquare className="msx-row-glyph" aria-hidden /><span className="msx-row-label">Chat</span>
                </button>
                <button type="button" className="msx-row" data-selected={selectedRow === 1} onClick={() => setSelectedRow(1)}>
                  <Terminal className="msx-row-glyph" aria-hidden /><span className="msx-row-label">Terminal</span>
                </button>
              </div>
            </Section>

            <Section title="Workspaces" count="4" defaultOpen>
              <div className="msx-ws-row">
                {WORKSPACES.map((w, i) => (
                  <button key={w} type="button" className="msx-ws" data-selected={selectedWs === i} onClick={() => setSelectedWs(i)}>
                    {w}
                    {i === 1 && <CardWork phase="streaming" />}
                    {i === 2 && <CardWork phase="thinking" />}
                  </button>
                ))}
              </div>
              <div style={{ display: "grid", gap: 1, padding: "0 0 4px" }}>
                <button type="button" className="msx-row" data-selected={selectedRow === 2} onClick={() => setSelectedRow(2)}>
                  <span className="msx-row-label">opus · shaping tokens</span>
                </button>
                <button type="button" className="msx-row" data-selected={selectedRow === 3} onClick={() => setSelectedRow(3)}>
                  <span className="msx-row-label">codex · chat panel</span>
                </button>
                <button type="button" className="msx-row msx-row-new">
                  <Plus className="msx-row-glyph" style={{ color: "var(--ct-accent)" }} aria-hidden />
                  <span className="msx-row-label">New session</span>
                </button>
              </div>
            </Section>

            <Section title="Recently Closed" count="2">
              <div style={{ display: "grid", gap: 1, padding: "2px 0 4px" }}>
                <button type="button" className="msx-row"><span className="msx-row-label" style={{ color: "#5b6672" }}>build.sh dev</span></button>
                <button type="button" className="msx-row"><span className="msx-row-label" style={{ color: "#5b6672" }}>git log</span></button>
              </div>
            </Section>
          </div>
          <p className="msx-caption">Sections reveal from their header (chevron on the same curve). Selection is a quiet fill — no decorative rail. Two workspaces are live: the genuine phase-dots, corner-sized.</p>
        </div>

        {/* ── Chat (the real surfaces) ────────────────────────── */}
        <div>
          <div className="msx-col-head"><span>Chat</span><span>working · executing · sending</span></div>
          <ChatSurface />
          <p className="msx-caption">Send a message → the bubble settles, the agent works (the real jumping dots), a command runs then finishes with a quiet completion sweep, and the answer streams under a block caret. Text never animates; motion only marks state.</p>
        </div>
      </div>
    </div>
  );
}
