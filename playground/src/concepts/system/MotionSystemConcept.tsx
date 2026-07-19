import { useState, type CSSProperties, type ReactNode } from "react";
import { ChevronRight, Plus, MoreHorizontal, Terminal, MessageSquare } from "lucide-react";
import { ChatSurface } from "./ChatSurface";
import "./motion-system.css";

const WORKSPACES = ["CT", "BN", "RK", "EQ"];

function Section({ title, count, defaultOpen, children }: { title: string; count?: string; defaultOpen?: boolean; children: ReactNode }) {
  const [open, setOpen] = useState(!!defaultOpen);
  return (
    <div>
      <button type="button" className="ms-disclosure" aria-expanded={open} onClick={() => setOpen((v) => !v)}>
        <ChevronRight className="ms-chevron" aria-hidden />
        {title}
        {count && <span className="ms-disclosure-count">{count}</span>}
      </button>
      <div className="ms-reveal" data-open={open}>
        <div className="ms-reveal-inner">{children}</div>
      </div>
    </div>
  );
}

// Stable per-instance phase offset — deterministic from index, held for life.
const phase = (i: number): CSSProperties => ({ animationDelay: `${-(i * 0.41).toFixed(2)}s` });

export function MotionSystemConcept() {
  const [selectedWs, setSelectedWs] = useState(0);
  const [selectedTab, setSelectedTab] = useState(1);
  const [menuOpen, setMenuOpen] = useState(false);
  const [modalOpen, setModalOpen] = useState(false);
  const [toasts, setToasts] = useState<{ id: number; leaving?: boolean }[]>([]);

  const fireToast = () => {
    const id = Date.now();
    setToasts((t) => [...t, { id }]);
    window.setTimeout(() => setToasts((t) => t.map((x) => (x.id === id ? { ...x, leaving: true } : x))), 2600);
    window.setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 2900);
  };

  return (
    <div className="ms">
      <div className="ms-stage">
        {/* ── Sidebar ─────────────────────────────────────────── */}
        <div>
          <div className="ms-col-head"><span>Sidebar</span><span>disclosure · selection · activity</span></div>
          <div className="ms-panel">
            <Section title="General" count="4">
              <div className="ms-rows">
                <button type="button" className="ms-row" data-selected={selectedTab === 0} onClick={() => setSelectedTab(0)}>
                  <MessageSquare className="ms-row-glyph" aria-hidden /><span className="ms-row-label">Chat</span>
                </button>
                <button type="button" className="ms-row" data-selected={selectedTab === 1} onClick={() => setSelectedTab(1)}>
                  <Terminal className="ms-row-glyph" aria-hidden /><span className="ms-row-label">Terminal</span>
                </button>
              </div>
            </Section>

            <Section title="Workspaces" count="4" defaultOpen>
              <div className="ms-ws-scroll">
                {WORKSPACES.map((w, i) => (
                  <button
                    key={w}
                    type="button"
                    className="ms-ws-card"
                    data-selected={selectedWs === i}
                    onClick={() => setSelectedWs(i)}
                  >
                    {w}
                    {(i === 1 || i === 2) && (
                      <span className={`ms-ws-activity ms-alive ${i === 1 ? "ms-amp-strong" : "ms-amp-quiet"}`} style={phase(i)} aria-hidden />
                    )}
                  </button>
                ))}
              </div>

              <div className="ms-rows">
                <div className="ms-row" style={{ cursor: "default", color: "var(--ms-faint)", fontSize: 10, letterSpacing: ".08em", textTransform: "uppercase" }}>
                  Motion Design · team
                </div>
                <button type="button" className="ms-row" data-selected={selectedTab === 2} onClick={() => setSelectedTab(2)}>
                  <span className="ms-row-dot ms-alive ms-amp-strong" style={phase(0)} aria-hidden />
                  <span className="ms-row-label">opus · shaping tokens</span>
                </button>
                <button type="button" className="ms-row" data-selected={selectedTab === 3} onClick={() => setSelectedTab(3)}>
                  <span className="ms-row-dot ms-alive ms-amp-quiet" style={phase(2)} aria-hidden />
                  <span className="ms-row-label">codex · chat panel</span>
                </button>
                <button type="button" className="ms-row ms-new" onClick={() => setModalOpen(true)}>
                  <Plus className="ms-row-glyph" style={{ color: "var(--ms-accent)" }} aria-hidden />
                  <span className="ms-row-label">New session</span>
                </button>
              </div>
            </Section>

            <Section title="Recently Closed" count="2">
              <div className="ms-rows">
                <button type="button" className="ms-row"><span className="ms-row-label" style={{ color: "var(--ms-faint)" }}>build.sh dev</span></button>
                <button type="button" className="ms-row"><span className="ms-row-label" style={{ color: "var(--ms-faint)" }}>git log</span></button>
              </div>
            </Section>
          </div>
          <p className="ms-caption">Select a card → the accent rail grows from it (causal, not a flood). Two agents work: same heartbeat period, offset phase, tiered amplitude — one calm room.</p>
        </div>

        {/* ── Chat (Keats' slice) ─────────────────────────────── */}
        <div>
          <div className="ms-col-head"><span>Chat</span><span>disclosure · send · streaming</span></div>
          <ChatSurface />
          <p className="ms-caption">Expand a command/tool block → it grows from its header. Send → the bubble settles in; the thread follows only when pinned to bottom. Streaming moves the heartbeat, never the text.</p>
        </div>

        {/* ── Overlays ────────────────────────────────────────── */}
        <div style={{ position: "relative" }}>
          <div className="ms-col-head"><span>Overlays</span><span>origin · tempo</span></div>
          <div className="ms-panel" style={{ padding: 12 }}>
            <div className="ms-overlay-demo">
              <div className="ms-menu-anchor">
                <button type="button" className="ms-btn ms-btn-ghost ms-press" style={{ width: "100%", justifyContent: "space-between" }} onClick={() => setMenuOpen((v) => !v)}>
                  Context menu <MoreHorizontal size={14} aria-hidden />
                </button>
                {menuOpen && (
                  <div className="ms-menu" role="menu" onMouseLeave={() => setMenuOpen(false)}>
                    <button type="button" className="ms-menu-item" role="menuitem" onClick={() => setMenuOpen(false)}>Rename</button>
                    <button type="button" className="ms-menu-item" role="menuitem" onClick={() => setMenuOpen(false)}>Duplicate</button>
                    <div className="ms-menu-sep" />
                    <button type="button" className="ms-menu-item" role="menuitem" data-danger="true" onClick={() => setMenuOpen(false)}>Close</button>
                  </div>
                )}
              </div>
              <button type="button" className="ms-btn ms-press" style={{ justifyContent: "center" }} onClick={fireToast}>Fire toast</button>
              <button type="button" className="ms-btn ms-btn-ghost ms-press" style={{ justifyContent: "center" }} onClick={() => setModalOpen(true)}>Open modal</button>
            </div>
          </div>
          <p className="ms-caption">The menu grows from its trigger corner. Toasts move at UI tempo, not a slower dialect. The modal is a true portal: backdrop fades, panel rises.</p>

          <div className="ms-toast-dock">
            {toasts.map((t) => (
              <div className="ms-toast" key={t.id} data-leaving={t.leaving} role="status">
                <span className="ms-toast-dot" /> Session created
              </div>
            ))}
          </div>
        </div>
      </div>

      {modalOpen && (
        <div className="ms-scrim" onClick={() => setModalOpen(false)}>
          <div className="ms-modal" role="dialog" aria-modal="true" onClick={(e) => e.stopPropagation()}>
            <h4>New session</h4>
            <p>Pick an agent and a working directory. The panel rose from center; the backdrop faded — you know exactly where it came from.</p>
            <div className="ms-modal-actions">
              <button type="button" className="ms-btn ms-btn-ghost ms-press" onClick={() => setModalOpen(false)}>Cancel</button>
              <button type="button" className="ms-btn ms-press" onClick={() => { setModalOpen(false); fireToast(); }}>Create</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
