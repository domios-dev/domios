// Caller: scripts/chat-scroll-trace/index.html (build.sh chat-scroll-trace)
import { useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { useMountedChatWindow } from "../../src/components/Terminal/useMountedChatWindow";
import { ScrollToLatestButton } from "../../src/components/Terminal/ScrollToLatestButton";
import { useChatScroll } from "../../src/components/Terminal/useChatScroll";

const fixed = new URLSearchParams(location.search).get("fix") !== "0";
const COUNT = 400;
const ROW_HEIGHTS = [44, 44, 380, 72, 44, 640, 96, 44, 260, 52];
const heightOf = (index: number) => ROW_HEIGHTS[(index * 7) % ROW_HEIGHTS.length] + (index % 3) * 6;
const initialRows = Array.from({ length: COUNT }, (_, index) => ({ msg: { id: `m${index}` } }));
type Grow = { index: number; px: number; timing: "commit" | "effect" };
const traceState = window as unknown as { chatScrollOwned: () => boolean; chatGrow: (grow: Grow) => void; chatAppend: (count: number) => void };

function Chat() {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  // New messages arrive at the tail, as a streamed turn adds rows.
  const [rows, setRows] = useState(initialRows);
  traceState.chatAppend = (count) => setRows(current => [...current,
    ...Array.from({ length: count }, (_, offset) => ({ msg: { id: `m${current.length + offset}` } }))]);
  // A row grows or shrinks like a tool card expanding: in the commit, or in an effect after paint as MessageBubble's
  // auto-expand does.
  const [extra, setExtra] = useState<Record<number, number>>({});
  const [pending, setPending] = useState<Grow | null>(null);
  const grow = (change: Grow) => setExtra(current => ({ ...current, [change.index]: (current[change.index] ?? 0) + change.px }));
  traceState.chatGrow = (change) => { if (change.timing === "effect") setPending(change); else grow(change); };
  useEffect(() => { if (pending) { grow(pending); setPending(null); } }, [pending]);
  const followTailRef = useRef(true);
  const userScrollingRef = useRef(false);
  traceState.chatScrollOwned = () => userScrollingRef.current;
  const win = useMountedChatWindow({ rows, scrollRef, active: true, followTailRef, sessionKey: "chat-scroll-trace",
    userScrollingRef: fixed ? userScrollingRef : undefined });
  const scroll = useChatScroll({ scrollRef, sessionKey: "chat-scroll-trace", active: true, loading: false, messageCount: rows.length,
    contentVersion: `${win.range.start}:${win.range.end}:${win.revision}`, prependVersion: "m0", followTailRef,
    onViewportChange: win.syncViewport, userScrollingRef: fixed ? userScrollingRef : undefined,
    foldLayoutDrift: fixed ? win.foldDrift : undefined });
  return (
    <>
    <div ref={scrollRef} id="scroller" onScroll={scroll.handleScroll}
      style={{ height: "100vh", overflowY: "auto", position: "relative" }}>
      <div ref={win.topSpacerRef} style={{ height: win.topSpacer }} />
      <div ref={win.rowsRef}>
      {rows.slice(win.range.start, win.range.end).map(({ msg }, offset) => {
        const index = win.range.start + offset;
        return (
          <div key={msg.id} data-chat-window-row={msg.id} data-message-id={msg.id} style={{ display: "flow-root" }}>
            <div style={{ height: heightOf(index) + (extra[index] ?? 0), margin: "4px 8px", borderRadius: 8, padding: 6, font: "12px sans-serif",
              background: index % 2 ? "#1d2735" : "#25303f", color: "#cbd5e1" }}>row {index}</div>
          </div>
        );
      })}
      </div>
      <div style={{ height: win.range.bottom }} />
    </div>
    <div id="jump" style={{ position: "fixed", right: 16, bottom: 16 }}>
      <ScrollToLatestButton testId="jump" gestureOwned={scroll.gestureOwned} userScrollingRef={userScrollingRef} onClick={scroll.scrollToBottom} />
    </div>
    </>
  );
}

const root = document.getElementById("root");
if (root) createRoot(root).render(<Chat />);
