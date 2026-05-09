import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { PeerTerminal } from "./PeerTerminal";
import { ConnectionLine } from "./ConnectionLine";
import type { Service } from "./types";
import type { PeerStatus } from "./useMeshData";

export interface RecentPush {
  source_id: string;
  target: string;
  at: number;
}

interface PeerLayout {
  name: string;
  ip: string;
  x: number;
  y: number;
  isThisMachine?: boolean;
  isPaired?: boolean;
  accessLevel?: string;
}

interface Props {
  thisMachine: { name: string; ip: string };
  peers: PeerStatus[];
  services: Service[];
  recentPushes: RecentPush[];
  onPeerClick: (peerName: string) => void;
  width: number;
  height: number;
}

const VOLUME_WINDOW_MS = 60 * 1000;
const PULSE_WINDOW_MS = 3000;
const MIN_SCALE = 0.4;
const MAX_SCALE = 2.5;
const ZOOM_STEP = 0.15;

export function MeshCanvas({
  thisMachine,
  peers,
  services,
  recentPushes,
  onPeerClick,
  width,
  height,
}: Props) {
  const layout = useMemo<PeerLayout[]>(() => {
    const cx = width / 2;
    const cy = height / 2;
    const radius = Math.max(150, Math.min(width, height) / 2 - 130);
    const items: PeerLayout[] = [
      {
        name: thisMachine.name,
        ip: thisMachine.ip,
        x: cx,
        y: cy,
        isThisMachine: true,
      },
    ];
    peers.forEach((p, i) => {
      const angle =
        (i / Math.max(peers.length, 1)) * Math.PI * 2 - Math.PI / 2;
      items.push({
        name: p.name,
        ip: p.ip,
        x: cx + Math.cos(angle) * radius,
        y: cy + Math.sin(angle) * radius,
        isPaired: true,
        accessLevel: p.access_level,
      });
    });
    return items;
  }, [thisMachine, peers, width, height]);

  const local = layout[0];
  const now = Date.now();

  const [transform, setTransform] = useState({ x: 0, y: 0, scale: 1 });
  const dragRef = useRef<{ startX: number; startY: number; baseX: number; baseY: number } | null>(null);
  const [isDragging, setIsDragging] = useState(false);
  const containerRef = useRef<HTMLDivElement | null>(null);

  const onMouseDown = (e: React.MouseEvent) => {
    if ((e.target as HTMLElement).closest("[data-peer-card]")) return;
    dragRef.current = {
      startX: e.clientX,
      startY: e.clientY,
      baseX: transform.x,
      baseY: transform.y,
    };
    setIsDragging(true);
  };

  useEffect(() => {
    if (!isDragging) return;
    const onMove = (e: MouseEvent) => {
      const d = dragRef.current;
      if (!d) return;
      setTransform((t) => ({ ...t, x: d.baseX + (e.clientX - d.startX), y: d.baseY + (e.clientY - d.startY) }));
    };
    const onUp = () => {
      dragRef.current = null;
      setIsDragging(false);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, [isDragging]);

  const onWheel = useCallback((e: React.WheelEvent) => {
    e.preventDefault();
    const delta = e.deltaY < 0 ? ZOOM_STEP : -ZOOM_STEP;
    setTransform((t) => {
      const nextScale = Math.max(MIN_SCALE, Math.min(MAX_SCALE, t.scale + delta));
      if (nextScale === t.scale) return t;
      const rect = containerRef.current?.getBoundingClientRect();
      if (!rect) return { ...t, scale: nextScale };
      const cx = e.clientX - rect.left;
      const cy = e.clientY - rect.top;
      const ratio = nextScale / t.scale;
      return {
        scale: nextScale,
        x: cx - (cx - t.x) * ratio,
        y: cy - (cy - t.y) * ratio,
      };
    });
  }, []);

  const reset = () => setTransform({ x: 0, y: 0, scale: 1 });

  return (
    <div
      ref={containerRef}
      className="relative w-full h-full overflow-hidden"
      onMouseDown={onMouseDown}
      onWheel={onWheel}
      style={{
        minHeight: height,
        background:
          "radial-gradient(ellipse at center, var(--ct-titlebar-bg) 0%, var(--ct-bg) 100%)",
        cursor: isDragging ? "grabbing" : "grab",
        userSelect: "none",
      }}
    >
      <div className="absolute top-2 right-2 z-10 flex gap-1 text-[10px]">
        <button
          onClick={() => setTransform((t) => ({ ...t, scale: Math.min(MAX_SCALE, t.scale + ZOOM_STEP) }))}
          className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded w-6 h-6 cursor-pointer text-[var(--ct-fg)] hover:border-[var(--ct-accent)]"
          title="Zoom in"
        >
          +
        </button>
        <button
          onClick={() => setTransform((t) => ({ ...t, scale: Math.max(MIN_SCALE, t.scale - ZOOM_STEP) }))}
          className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded w-6 h-6 cursor-pointer text-[var(--ct-fg)] hover:border-[var(--ct-accent)]"
          title="Zoom out"
        >
          −
        </button>
        <button
          onClick={reset}
          className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-2 h-6 cursor-pointer text-[var(--ct-fg)] hover:border-[var(--ct-accent)] text-[9px]"
          title="Reset view"
        >
          fit
        </button>
      </div>
      <div
        style={{
          transform: `translate(${transform.x}px, ${transform.y}px) scale(${transform.scale})`,
          transformOrigin: "0 0",
          width: width,
          height: height,
          position: "relative",
        }}
      >
        <svg
          className="absolute inset-0 pointer-events-none"
          width={width}
          height={height}
        >
          {layout.slice(1).map((p, i) => {
            const peerPushes = recentPushes.filter(
              (rp) =>
                rp.source_id.includes(p.name) || rp.target.includes(p.name),
            );
            const pushCount = peerPushes.filter(
              (rp) => now - rp.at < VOLUME_WINDOW_MS,
            ).length;
            const pulsing = peerPushes.some(
              (rp) => now - rp.at < PULSE_WINDOW_MS,
            );
            return (
              <ConnectionLine
                key={`${p.name}-${i}`}
                fromX={local.x}
                fromY={local.y}
                toX={p.x}
                toY={p.y}
                pushCount={pushCount}
                pulsing={pulsing}
              />
            );
          })}
        </svg>
        {layout.map((p, i) => (
          <div
            key={`${p.name}-${i}`}
            data-peer-card
            style={{
              position: "absolute",
              left: p.x,
              top: p.y,
              transform: "translate(-50%,-50%)",
            }}
          >
            <PeerTerminal
              name={p.name}
              ip={p.ip}
              isThisMachine={p.isThisMachine}
              isPaired={p.isPaired}
              accessLevel={p.accessLevel as "full" | "read_only" | "agent_isolated" | undefined}
              services={services.filter((s) =>
                p.isThisMachine ? !s.source_peer : s.source_peer === p.name,
              )}
              onClick={() => onPeerClick(p.name)}
            />
          </div>
        ))}
      </div>
    </div>
  );
}
