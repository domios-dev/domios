import { useEffect, useMemo, useRef } from "react";
import { Plus } from "lucide-react";
import { PeerTerminal } from "./PeerTerminal";
import { ConnectionLine } from "./ConnectionLine";
import type { Service } from "./types";
import type { PeerStatus } from "./useMeshData";
import { useCanvasViewport, formatLayerTransform } from "../Canvas/useCanvasViewport";
import { webLinkWithTokenAndRelay } from "../../lib/webAccess";

export interface RecentPush {
  source_id: string;
  target: string;
  at: number;
}

interface PeerLayout {
  id: string;
  name: string;
  ip: string;
  x: number;
  y: number;
  isThisMachine?: boolean;
  isPaired?: boolean;
  accessLevel?: string;
  isGhost?: boolean;
  servicesStale?: boolean;
  servicesError?: string | null;
  servicesLoaded?: boolean;
  webUrl?: string | null;
}

interface PushStats {
  pushCount: number;
  pulsing: boolean;
}

interface Props {
  thisMachine: { name: string; ip: string };
  peers: PeerStatus[];
  services: Service[];
  recentPushes: RecentPush[];
  onPeerClick: (peerName: string) => void;
  width: number;
  height: number;
  onAddPeerClick?: () => void;
  meshEnabled?: boolean;
  onGoOnline?: () => void;
  onOpenPeerWebUi?: (url: string) => void;
  webUiBaseUrl?: string | null;
  webUiToken?: string | null;
}

const VOLUME_WINDOW_MS = 60 * 1000;
const PULSE_WINDOW_MS = 3000;
const MIN_SCALE = 0.25;
const MAX_SCALE = 2.5;
const ZOOM_STEP = 0.06;
const FIT_PADDING = 80;
const CARD_HALF_WIDTH = 180;
const CARD_HALF_HEIGHT = 80;
const CARD_GAP = 60;
const CARD_FULL_WIDTH = CARD_HALF_WIDTH * 2;

export function MeshCanvas({
  thisMachine,
  peers,
  services,
  recentPushes,
  onPeerClick,
  width,
  height,
  onAddPeerClick,
  meshEnabled = true,
  onGoOnline,
  onOpenPeerWebUi,
  webUiBaseUrl = null,
  webUiToken = null,
}: Props) {
  const showGhost = peers.length === 0 && !!onAddPeerClick;
  const layout = useMemo<PeerLayout[]>(() => {
    const cx = width / 2;
    const cy = height / 2;
    const cardSpacing = CARD_FULL_WIDTH + CARD_GAP;
    // Clear the center "this machine" card horizontally: half-center + half-peer + gap.
    const minRadius = CARD_FULL_WIDTH + CARD_GAP;
    const dynamicRadius = peers.length > 0 ? cardSpacing / (2 * Math.sin(Math.PI / Math.max(peers.length, 3))) : minRadius;
    const radius = Math.max(minRadius, dynamicRadius);
    const items: PeerLayout[] = [
      {
        id: "__self__",
        name: thisMachine.name,
        ip: thisMachine.ip,
        x: cx,
        y: cy,
        isThisMachine: true,
        webUrl: webUiBaseUrl
          ? webLinkWithTokenAndRelay(webUiBaseUrl, webUiToken, null)
          : null,
      },
    ];
    peers.forEach((p, i) => {
      const angle =
        (i / Math.max(peers.length, 1)) * Math.PI * 2 - Math.PI / 2;
      items.push({
        id: p.instance_id,
        name: p.name,
        ip: p.ip,
        x: cx + Math.cos(angle) * radius,
        y: cy + Math.sin(angle) * radius,
        isPaired: true,
        accessLevel: p.access_level,
        servicesStale: p.services_stale,
        servicesError: p.services_error,
        servicesLoaded: p.services_loaded,
        webUrl: p.online && webUiBaseUrl
          ? webLinkWithTokenAndRelay(webUiBaseUrl, webUiToken, {
              id: p.instance_id,
              name: p.name,
            })
          : null,
      });
    });
    if (showGhost) {
      items.push({
        id: "__ghost_add_peer__",
        name: "__ghost_add_peer__",
        ip: "",
        x: cx + radius,
        y: cy,
        isGhost: true,
      });
    }
    return items;
  }, [thisMachine, peers, width, height, showGhost, webUiBaseUrl, webUiToken]);
  const layoutFitKey = useMemo(
    () =>
      [
        `${Math.round(width)}x${Math.round(height)}`,
        `${thisMachine.name}@${thisMachine.ip}`,
        showGhost ? "ghost" : "no-ghost",
        peers
          .map((p) => `${p.instance_id}:${p.name}@${p.ip}:${p.access_level ?? ""}:${p.services_stale ?? ""}:${p.services_error ?? ""}`)
          .sort()
          .join("|"),
      ].join("::"),
    [width, height, thisMachine.name, thisMachine.ip, showGhost, peers],
  );

  const local = layout[0];
  const remoteLayout = useMemo(() => layout.slice(1), [layout]);
  const serviceByPeer = useMemo(() => {
    const grouped = new Map<string, Service[]>();
    grouped.set("__self__", []);
    for (const s of services) {
      const key = s.source_peer_id || s.source_peer || "__self__";
      const list = grouped.get(key);
      if (list) list.push(s);
      else grouped.set(key, [s]);
    }
    return grouped;
  }, [services]);
  const pushStatsByPeer = useMemo(() => {
    const now = Date.now();
    const stats = new Map<string, PushStats>();
    for (const p of peers) stats.set(p.name, { pushCount: 0, pulsing: false });
    for (const rp of recentPushes) {
      const inVolumeWindow = now - rp.at < VOLUME_WINDOW_MS;
      const inPulseWindow = now - rp.at < PULSE_WINDOW_MS;
      if (!inVolumeWindow && !inPulseWindow) continue;
      for (const p of peers) {
        if (!rp.source_id.includes(p.name) && !rp.target.includes(p.name)) continue;
        const stat = stats.get(p.name);
        if (!stat) continue;
        if (inVolumeWindow) stat.pushCount++;
        if (inPulseWindow) stat.pulsing = true;
      }
    }
    return stats;
  }, [peers, recentPushes]);
  const peerLinks = useMemo(
    () =>
      remoteLayout.flatMap((a, i) =>
        remoteLayout.slice(i + 1).map((b, j) => ({ a, b, key: `p2p-${a.name}-${b.name}-${i}-${j}` })),
      ),
    [remoteLayout],
  );

  const { transform, handlers, fitToView, zoomBy, isDragging, containerRef, layerRef } = useCanvasViewport({
    width,
    height,
    minScale: MIN_SCALE,
    maxScale: MAX_SCALE,
    fitPadding: FIT_PADDING,
    ignoreSelector: "[data-peer-card]",
    wheelBehavior: "zoom",
  });
  const lastAutoFitKeyRef = useRef<string | null>(null);

  const contentBounds = useMemo(() => {
    if (!layout.length) return null;
    const minX = Math.min(...layout.map((p) => p.x - CARD_HALF_WIDTH));
    const maxX = Math.max(...layout.map((p) => p.x + CARD_HALF_WIDTH));
    const minY = Math.min(...layout.map((p) => p.y - CARD_HALF_HEIGHT));
    const maxY = Math.max(...layout.map((p) => p.y + CARD_HALF_HEIGHT));
    return { x: minX, y: minY, width: maxX - minX, height: maxY - minY };
  }, [layout]);

  useEffect(() => {
    if (!contentBounds || width <= 0 || height <= 0) return;
    if (lastAutoFitKeyRef.current === layoutFitKey) return;
    lastAutoFitKeyRef.current = layoutFitKey;
    fitToView(contentBounds);
  }, [fitToView, contentBounds, layoutFitKey, width, height]);

  const reset = () => {
    if (contentBounds) fitToView(contentBounds);
  };

  return (
    <div
      ref={containerRef}
      className="relative w-full h-full overflow-hidden"
      onMouseDown={handlers.onMouseDown}
      style={{
        minHeight: height,
        background:
          "radial-gradient(ellipse at center, var(--ct-titlebar-bg) 0%, var(--ct-bg) 100%)",
        cursor: isDragging ? "grabbing" : "grab",
        userSelect: "none",
        touchAction: "none",
      }}
    >
      <div className="absolute top-2 right-2 z-10 flex gap-1 text-[10px]">
        <button
          onClick={() => zoomBy(ZOOM_STEP)}
          className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded w-6 h-6 cursor-pointer text-[var(--ct-fg)] hover:border-[var(--ct-accent)]"
          title="Zoom in"
        >
          +
        </button>
        <button
          onClick={() => zoomBy(-ZOOM_STEP)}
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
        ref={layerRef}
        style={{
          transform: formatLayerTransform(transform),
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
          style={{ overflow: "visible" }}
        >
          {remoteLayout.map((p, i) => {
            if (p.isGhost) {
              return (
                <ConnectionLine
                  key={`hub-ghost-${i}`}
                  fromX={local.x}
                  fromY={local.y}
                  toX={p.x}
                  toY={p.y}
                  pushCount={0}
                  pulsing={false}
                  isGhost
                />
              );
            }
            const stats = pushStatsByPeer.get(p.name);
            return (
              <ConnectionLine
                key={`hub-${p.name}-${i}`}
                fromX={local.x}
                fromY={local.y}
                toX={p.x}
                toY={p.y}
                pushCount={stats?.pushCount ?? 0}
                pulsing={stats?.pulsing ?? false}
              />
            );
          })}
          {peerLinks.map(({ a, b, key }) => {
              if (a.isGhost || b.isGhost) return null;
              return (
                <ConnectionLine
                  key={key}
                  fromX={a.x}
                  fromY={a.y}
                  toX={b.x}
                  toY={b.y}
                  pushCount={0}
                  pulsing={false}
                />
              );
            })}
        </svg>
        {layout.map((p, i) => (
          <div
            key={`${p.id}-${i}`}
            data-peer-card
            style={{
              position: "absolute",
              left: p.x,
              top: p.y,
              transform: "translate(-50%,-50%)",
            }}
          >
            {p.isGhost ? (
              <button
                onClick={onAddPeerClick}
                className="w-[260px] py-6 rounded-xl border-2 border-dashed border-[var(--ct-border)] bg-transparent text-[var(--ct-muted-fg)] hover:border-[var(--ct-accent)] hover:text-[var(--ct-fg)] flex flex-col items-center gap-1 cursor-pointer transition-colors"
                data-peer-card
              >
                <Plus size={20} />
                <span className="text-[11px] uppercase tracking-wider">Add a peer</span>
                <span className="text-[10px] opacity-70 normal-case tracking-normal">discover or invite</span>
              </button>
            ) : (
              <PeerTerminal
                name={p.name}
                ip={p.ip}
                isThisMachine={p.isThisMachine}
                isPaired={p.isPaired}
                accessLevel={p.accessLevel as "full" | "read_only" | "agent_isolated" | undefined}
                services={serviceByPeer.get(p.id) ?? []}
                onClick={() => onPeerClick(p.name)}
                offline={p.isThisMachine && !meshEnabled}
                onGoOnline={p.isThisMachine ? onGoOnline : undefined}
                servicesStale={p.servicesStale}
                servicesError={p.servicesError}
                servicesLoaded={p.servicesLoaded}
                webUrl={p.webUrl}
                onOpenWebUi={onOpenPeerWebUi}
              />
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
