import { useEffect, useRef, useState, type CSSProperties } from "react";
import {
  ChevronRight, Terminal, Copy, RotateCcw, CheckCircle2, Loader, Search, Check,
} from "lucide-react";

type Phase = "thinking" | "streaming" | "work";
type Turn =
  | { kind: "user"; id: number; text: string; settling?: boolean }
  | { kind: "assistant"; id: number; text: string; streaming?: boolean }
  | { kind: "working"; id: number; label: string; phase: Phase }
  | { kind: "command"; id: number; text: string; cwd: string; status: "running" | "ok"; flash?: boolean }
  | { kind: "tool"; id: number; label: string; summary: string; body: string; open: boolean };

let uid = 1;
const nid = () => uid++;

const INITIAL: Turn[] = [
  { kind: "user", id: nid(), text: "What changed in the sidebar disclosure?" },
  { kind: "tool", id: nid(), label: "Search", summary: "\"sidebar-disclosure\" · 3 files", open: false,
    body: "src/components/Sidebar/SidebarDisclosure.tsx:16\nsrc/styles/globals.css:1527\nsrc/components/Sidebar/TeamUnit.tsx:541" },
  { kind: "assistant", id: nid(), text: "The chevron rotates, but the section body just mounts — no reveal. Let me check the CSS." },
];

export function ChatSurface() {
  const [turns, setTurns] = useState<Turn[]>(INITIAL);
  const threadRef = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);
  const [draft, setDraft] = useState("");

  const atBottom = () => {
    const el = threadRef.current;
    return !el || el.scrollHeight - el.scrollTop - el.clientHeight < 28;
  };
  useEffect(() => {
    if (pinned.current) threadRef.current?.scrollTo({ top: threadRef.current.scrollHeight, behavior: "smooth" });
  }, [turns]);

  const patch = (id: number, p: Partial<Turn>) =>
    setTurns((t) => t.map((x) => (x.id === id ? ({ ...x, ...p } as Turn) : x)));
  const add = (t: Turn) => setTurns((prev) => [...prev, t]);

  // Simulate an execution: working → command running → done (flash) → answer.
  const runDemo = () => {
    pinned.current = true;
    const workId = nid();
    add({ kind: "working", id: workId, label: "Reading globals.css", phase: "work" });
    const cmdId = nid();
    window.setTimeout(() => {
      patch(workId, { label: "Running check", phase: "thinking" } as Partial<Turn>);
      add({ kind: "command", id: cmdId, text: "rg -n grid-template-rows src/styles/globals.css", cwd: "~/codeterm", status: "running" });
    }, 900);
    window.setTimeout(() => patch(cmdId, { status: "ok", flash: true } as Partial<Turn>), 2200);
    window.setTimeout(() => patch(workId, { label: "Writing the fix", phase: "streaming" } as Partial<Turn>), 2500);
    window.setTimeout(() => {
      setTurns((t) => t.filter((x) => x.id !== workId));
      add({ kind: "assistant", id: nid(), text: "Fixed — the body now reveals via a `grid-template-rows` transition, and the chevron rotates on the same curve, so header and body read as one motion.", streaming: true });
    }, 3400);
  };

  const send = () => {
    const text = draft.trim();
    if (!text) return;
    pinned.current = true;
    const id = nid();
    add({ kind: "user", id, text, settling: true });
    setDraft("");
    requestAnimationFrame(() => patch(id, { settling: false } as Partial<Turn>));
    window.setTimeout(runDemo, 320);
  };

  return (
    <div className="msx-panel msx-chat">
      <div className="msx-thread" ref={threadRef} onScroll={() => { pinned.current = atBottom(); }}>
        {turns.map((t) => {
          if (t.kind === "user") {
            return (
              <div className="msx-turn msx-user" key={t.id} data-settling={t.settling ? "true" : undefined}>
                <div className="msx-bubble">{t.text}</div>
              </div>
            );
          }
          if (t.kind === "assistant") {
            return (
              <div className="msx-turn" key={t.id}>
                <div className="msx-assistant">
                  {t.text}
                  {t.streaming && <span className="msx-caret" aria-hidden />}
                </div>
              </div>
            );
          }
          if (t.kind === "working") {
            return (
              <div className="msx-turn" key={t.id}>
                <div className="ct-agent-row" data-agent-state="working" data-agent-phase={t.phase} role="status">
                  <span aria-hidden className="ct-phase-dots"><i /><i /><i /></span>
                  <span className="ct-agent-msg" style={{ fontFamily: "var(--ct-font-mono)", fontSize: 13 } as CSSProperties}>{t.label}…</span>
                </div>
              </div>
            );
          }
          if (t.kind === "command") {
            const running = t.status === "running";
            return (
              <div className="msx-turn" key={t.id}>
                <div className={`msx-cmd${t.flash ? " msx-flash" : ""}`} data-status={t.status} data-tone="accent">
                  {running
                    ? <Loader className="msx-cmd-glyph msx-spin" style={{ stroke: "var(--ct-muted-fg)" }} aria-label="running" />
                    : <CheckCircle2 className="msx-cmd-glyph msx-pop" key="ok" style={{ stroke: "var(--ct-green)" }} aria-label="ok" />}
                  <Terminal className="msx-cmd-term" aria-hidden />
                  <span className="msx-cmd-text" title={t.text}>{t.text}</span>
                  <span className="msx-cmd-cwd">{t.cwd}</span>
                  <span className="msx-cmd-actions">
                    <button type="button" className="msx-icon-btn" title="Copy"><Copy size={12} /></button>
                    <button type="button" className="msx-icon-btn" title="Re-run"><RotateCcw size={12} /></button>
                    <button type="button" className="msx-icon-btn" title="Jump to output"><Terminal size={12} /></button>
                  </span>
                </div>
              </div>
            );
          }
          // tool card
          return (
            <div className="msx-turn" key={t.id}>
              <div className="msx-tool" data-open={t.open ? "true" : "false"}>
                <button type="button" className="msx-tool-head" aria-expanded={t.open} onClick={() => patch(t.id, { open: !t.open } as Partial<Turn>)}>
                  <span className="msx-tool-chip" style={{ color: "var(--ct-accent)", background: "color-mix(in srgb, var(--ct-accent) 15%, transparent)" }}>
                    <Search size={14} strokeWidth={2} />
                  </span>
                  <span className="msx-tool-title">
                    <span className="msx-tool-label" style={{ color: "var(--ct-accent)" }}>{t.label}</span>
                    <span className="msx-tool-summary">{t.summary}</span>
                  </span>
                  <span className="msx-tool-status" style={{ color: "var(--ct-green)" }}><Check size={14} /></span>
                  <ChevronRight className="msx-tool-chevron" aria-hidden />
                </button>
                <div className="msx-tool-body"><div><pre>{t.body}</pre></div></div>
              </div>
            </div>
          );
        })}
      </div>
      <div className="msx-composer">
        <button type="button" className="msx-icon-btn" title="Replay the agent run" onClick={runDemo} style={{ padding: "8px 10px", border: "1px solid var(--ct-border)" }}>
          <RotateCcw size={13} />
        </button>
        <input
          value={draft}
          placeholder="Message the agent…"
          onChange={(e) => setDraft(e.currentTarget.value)}
          onKeyDown={(e) => { if (e.key === "Enter") send(); }}
        />
        <button type="button" className="msx-send-btn" onClick={send}>Send</button>
      </div>
    </div>
  );
}
