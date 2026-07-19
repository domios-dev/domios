import { useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { ChevronRight } from "lucide-react";
import "./chat-surface.css";

type Msg =
  | { id: number; role: "user"; text: string; settling?: boolean }
  | { id: number; role: "assistant"; text: string; streaming?: boolean }
  | { id: number; role: "block"; tag: string; title: string; body: string };

const SEED: Msg[] = [
  { id: 1, role: "user", text: "Refactor the sidebar disclosure into one primitive." },
  { id: 2, role: "assistant", text: "On it. Reading the current implementation.", streaming: true },
  {
    id: 3,
    role: "block",
    tag: "command",
    title: "rg -n \"sidebar-disclosure\" src/",
    body: "src/components/Sidebar/SidebarDisclosure.tsx:16\nsrc/styles/globals.css:1527\nsrc/components/Sidebar/TeamUnit.tsx:541",
  },
  {
    id: 4,
    role: "block",
    tag: "tool",
    title: "read_file · SidebarDisclosure.tsx",
    body: "export function SidebarDisclosure({ expanded, children }) {\n  return (\n    <button aria-expanded={expanded}>…</button>\n  );\n}",
  },
];

let nextId = 100;

export function ChatSurface() {
  const [messages, setMessages] = useState<Msg[]>(SEED);
  const [open, setOpen] = useState<Record<number, boolean>>({ 3: true });
  const [draft, setDraft] = useState("");
  const threadRef = useRef<HTMLDivElement>(null);
  const pinnedRef = useRef(true);

  const isPinned = () => {
    const el = threadRef.current;
    if (!el) return true;
    return el.scrollHeight - el.scrollTop - el.clientHeight < 24;
  };

  useEffect(() => {
    if (pinnedRef.current) threadRef.current?.scrollTo({ top: threadRef.current.scrollHeight, behavior: "smooth" });
  }, [messages]);

  const send = () => {
    const text = draft.trim();
    if (!text) return;
    pinnedRef.current = true;
    const id = nextId++;
    setMessages((m) => [...m, { id, role: "user", text, settling: true }]);
    setDraft("");
    window.requestAnimationFrame(() =>
      setMessages((m) => m.map((x) => (x.id === id ? { ...x, settling: false } : x))),
    );
  };

  const rendered = useMemo(() => messages, [messages]);

  return (
    <div className="ms-panel ms-chat">
      <div
        className="ms-chat-thread"
        ref={threadRef}
        onScroll={() => {
          pinnedRef.current = isPinned();
        }}
      >
        {rendered.map((m) => {
          if (m.role === "block") {
            const isOpen = !!open[m.id];
            return (
              <div className="ms-block" key={m.id} data-open={isOpen}>
                <button
                  type="button"
                  className="ms-block-head"
                  aria-expanded={isOpen}
                  onClick={() => setOpen((o) => ({ ...o, [m.id]: !o[m.id] }))}
                >
                  <ChevronRight
                    className="ms-chevron"
                    style={{ transform: isOpen ? "rotate(90deg)" : undefined } as CSSProperties}
                    aria-hidden
                  />
                  <span className="ms-block-tag">{m.tag}</span>
                  <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                    {m.title}
                  </span>
                </button>
                <div className="ms-block-body">
                  <pre>{m.body}</pre>
                </div>
              </div>
            );
          }
          return (
            <div className="ms-msg" key={m.id} data-role={m.role} data-settling={m.role === "user" && m.settling}>
              <span className="ms-msg-role">{m.role}</span>
              <div className="ms-bubble">
                {m.text}
                {m.role === "assistant" && m.streaming && (
                  <span className="ms-stream-dots" aria-label="streaming">
                    <i /><i style={{ animationDelay: "-0.9s" } as CSSProperties} /><i style={{ animationDelay: "-1.8s" } as CSSProperties} />
                  </span>
                )}
              </div>
            </div>
          );
        })}
      </div>
      <div className="ms-composer">
        <input
          value={draft}
          placeholder="Message the agent…"
          onChange={(e) => setDraft(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") send();
          }}
        />
        <button type="button" className="ms-btn ms-press" onClick={send}>Send</button>
      </div>
    </div>
  );
}
