// Caller: scripts/chat-scroll-trace/index.html (build.sh chat-scroll-trace)
import { useRef } from "react";
import { createRoot } from "react-dom/client";
import { useMountedChatWindow } from "../../src/components/Terminal/useMountedChatWindow";
import { useChatScroll } from "../../src/components/Terminal/useChatScroll";

const fixed = new URLSearchParams(location.search).get("fix") !== "0";
const COUNT = 400;
const ROW_HEIGHTS = [44, 44, 380, 72, 44, 640, 96, 44, 260, 52];
const heightOf = (index: number) => ROW_HEIGHTS[(index * 7) % ROW_HEIGHTS.length] + (index % 3) * 6;
const rows = Array.from({ length: COUNT }, (_, index) => ({ msg: { id: `m${index}` } }));

function Chat() {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const followTailRef = useRef(true);
  const userScrollingRef = useRef(false);
  const win = useMountedChatWindow({ rows, scrollRef, active: true, followTailRef, sessionKey: "chat-scroll-trace",
    userScrollingRef: fixed ? userScrollingRef : undefined });
  const scroll = useChatScroll({ scrollRef, sessionKey: "chat-scroll-trace", active: true, loading: false, messageCount: COUNT,
    contentVersion: `${win.range.start}:${win.range.end}:${win.revision}`, prependVersion: "m0", followTailRef,
    onViewportChange: win.syncViewport, userScrollingRef: fixed ? userScrollingRef : undefined,
    foldLayoutDrift: fixed ? win.foldDrift : undefined });
  return (
    <div ref={scrollRef} id="scroller" onScroll={scroll.handleScroll}
      style={{ height: "100vh", overflowY: "auto", position: "relative" }}>
      <div ref={win.topSpacerRef} style={{ height: win.topSpacer }} />
      {rows.slice(win.range.start, win.range.end).map(({ msg }, offset) => {
        const index = win.range.start + offset;
        return (
          <div key={msg.id} data-chat-window-row={msg.id} data-message-id={msg.id} style={{ display: "flow-root" }}>
            <div style={{ height: heightOf(index), margin: "4px 8px", borderRadius: 8, padding: 6, font: "12px sans-serif",
              background: index % 2 ? "#1d2735" : "#25303f", color: "#cbd5e1" }}>row {index}</div>
          </div>
        );
      })}
      <div style={{ height: win.range.bottom }} />
    </div>
  );
}

const root = document.getElementById("root");
if (root) createRoot(root).render(<Chat />);
