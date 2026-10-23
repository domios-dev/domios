import { peerInventoryModel } from "./peerInventoryModel";
import type { Service } from "./types";
import { memo, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ArrowRight, Globe, Plus } from "lucide-react";
import { PeerTerminal } from "./PeerTerminal";
import { meshEdgeModel } from "./meshEdgeModel";
import type { PeerStatus } from "./useMeshData";
import { CanvasToolbar } from "@codeterm/chat-ui";
import { useCanvasViewport, formatLayerTransform } from "@codeterm/canvas-camera";
import { webLinkWithTokenAndRelay } from "../../lib/webAccess";
import { RELAY_PROTOCOL_VERSION } from "../../lib/types";
import { webGraphLayout, webGraphInitialView, WEB_CARD_SCALE, MACHINE_CARD_SCALE, MACHINE_CARD_WIDTH } from "./webGraphLayout";
import { webGatewayGraph } from "./meshProjection";

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
  height: number;
  isThisMachine?: boolean;
  isPaired?: boolean;
  pending?: boolean;
  accessLevel?: string;
  isGhost?: boolean;
  webUrl?: string | null;
  webUnavailableReason?: string;
}

interface Props {
  webCard?: (qrSize: number) => ReactNode;
  onRefresh?: () => void;
  onWebClick?: () => void;
  webSelected?: boolean;
  webStatus?: string;
  thisMachine: { name: string; ip: string };
  peers: PeerStatus[];
  services?: Service[];
  loading?: boolean;
  error?: string | null;
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

const MIN_SCALE = 0.001;
const MAX_SCALE = 2.5;
const ZOOM_STEP = 0.06;
const FIT_PADDING = 24;
const CARD_HALF_WIDTH = MACHINE_CARD_WIDTH / 2;

export const MeshCanvas = memo(function MeshCanvas({
  webCard,
  onRefresh,
  onWebClick,
  webSelected = false,
  webStatus = "Unavailable",
  thisMachine,
  peers,
  services = [],
  loading = false,
  error,
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
  const webCardRef = useRef<HTMLDivElement>(null);
  const [webHeight, setWebHeight] = useState(800);
  const [machineHeights, setMachineHeights] = useState<number[]>([]);
  const machineLayerRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const element = webCardRef.current;
    if (!element) return;
    if (element.offsetHeight > 0) setWebHeight(element.offsetHeight);
    const observer = new ResizeObserver(() => { if (element.offsetHeight > 0) setWebHeight(element.offsetHeight); });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  const geometry = webGraphLayout(width, height, peers.length + 1, webHeight, machineHeights);
  const showGhost = peers.length === 0 && !!onAddPeerClick;
  const layout = useMemo<PeerLayout[]>(() => {
    const cx = width / 2;
    const cy = height / 2;
    const items: PeerLayout[] = [
      {
        id: "__self__",
        name: thisMachine.name,
        ip: thisMachine.ip,
        ...geometry.machines[0],
        isThisMachine: true,
        webUrl: webUiBaseUrl
          ? webLinkWithTokenAndRelay(webUiBaseUrl, webUiToken, null)
          : null,
        webUnavailableReason: webUiBaseUrl ? undefined : "Web UI is not enabled on this machine",
      },
    ];
    peers.forEach((p, i) => {
      items.push({
        id: p.instance_id,
        name: p.name,
        ip: p.ip,
        ...geometry.machines[i + 1],
        // A discovered-but-not-paired (vouched) peer must never render as paired (codeterm#440).
        isPaired: !p.pending,
        pending: p.pending,
        accessLevel: p.access_level,
        webUrl: p.online && p.relay_protocol_version === RELAY_PROTOCOL_VERSION && webUiBaseUrl
          ? webLinkWithTokenAndRelay(webUiBaseUrl, webUiToken, {
              id: p.instance_id,
            })
          : null,
        webUnavailableReason: !p.online
          ? `${p.name} is offline`
          : p.relay_protocol_version !== RELAY_PROTOCOL_VERSION
            ? `Update Domios on ${p.name} to use Web UI relay`
            : "The browser gateway is unavailable",
      });
    });
    if (showGhost) {
      items.push({
        id: "__ghost_add_peer__",
        name: "__ghost_add_peer__",
        ip: "",
        x: cx + geometry.web.width / 2 + MACHINE_CARD_WIDTH / 2 + 20,
        y: cy,
        height: 88,
        isGhost: true,
      });
    }
    return items;
  }, [thisMachine, peers, width, height, showGhost, webUiBaseUrl, webUiToken, geometry.machines, geometry.web.width]);

  useLayoutEffect(() => {
    const nodes = Array.from(machineLayerRef.current?.querySelectorAll<HTMLElement>("[data-machine-index]") ?? []);
    const measure = () => {
      const heights = nodes.map((node) => node.offsetHeight * MACHINE_CARD_SCALE);
      if (heights.some((height) => height <= 0)) return;
      setMachineHeights((current) => heights.length === current.length && heights.every((height, index) => height === current[index]) ? current : heights);
    };
    measure();
    const observer = new ResizeObserver(measure);
    nodes.forEach((node) => observer.observe(node));
    return () => observer.disconnect();
  }, [peers.length]);

  const gateway = { x: width / 2, y: height / 2 };
  const gatewayGraph = webGatewayGraph(peers, meshEnabled);
  const { transform, handlers, fitToView, zoomBy, isDragging, containerRef, layerRef } = useCanvasViewport({
    width,
    height,
    minScale: MIN_SCALE,
    maxScale: MAX_SCALE,
    fitPadding: FIT_PADDING,
    ignoreSelector: "[data-peer-card]",
    wheelBehavior: "zoom",
  });
  const userAdjustedRef = useRef(false);
  const fittedBoundsRef = useRef("");

  const contentBounds = useMemo(() => {
    if (!layout.length) return null;
    const minX = Math.min(gateway.x - geometry.web.width / 2, ...layout.map((p) => p.x - CARD_HALF_WIDTH));
    const maxX = Math.max(gateway.x + geometry.web.width / 2, ...layout.map((p) => p.x + CARD_HALF_WIDTH));
    const minY = Math.min(gateway.y - webHeight / 2, ...layout.map((p) => p.y - p.height / 2));
    const maxY = Math.max(gateway.y + webHeight / 2, ...layout.map((p) => p.y + p.height / 2));
    const halfWidth = Math.max(gateway.x - minX, maxX - gateway.x);
    const halfHeight = Math.max(gateway.y - minY, maxY - gateway.y);
    return { x: gateway.x - halfWidth, y: gateway.y - halfHeight, width: halfWidth * 2, height: halfHeight * 2 };
  }, [layout, gateway.x, gateway.y, geometry.web.width, webHeight, webCard, geometry.web, geometry.qrSize]);

  useLayoutEffect(() => {
    if (!contentBounds) return;
    const key = [width, height, contentBounds.x, contentBounds.y, contentBounds.width, contentBounds.height].join(":");
    const initial = webGraphInitialView(width, height, contentBounds, userAdjustedRef.current);
    if (!initial || fittedBoundsRef.current === key) return;
    fittedBoundsRef.current = key;
    fitToView(initial.bounds);
  }, [fitToView, contentBounds, width, height]);

  const reset = () => {
    if (contentBounds) fitToView(contentBounds);
  };

  return (
    <div
      ref={containerRef}
      className="relative w-full h-full overflow-hidden"
      onMouseDown={(event) => { userAdjustedRef.current = true; handlers.onMouseDown(event); }}
      onWheelCapture={() => { userAdjustedRef.current = true; }}
      onTouchStartCapture={() => { userAdjustedRef.current = true; }}
      style={{
        minHeight: height,
        background:
          "radial-gradient(ellipse at center, var(--ct-titlebar-bg) 0%, var(--ct-bg) 100%)",
        cursor: isDragging ? "grabbing" : "grab",
        userSelect: "none",
        touchAction: "none",
      }}
    >
      <CanvasToolbar label="Mesh graph controls" onRefresh={onRefresh} onFit={() => { userAdjustedRef.current = true; reset(); }} onZoomOut={() => { userAdjustedRef.current = true; zoomBy(-ZOOM_STEP); }} onZoomIn={() => { userAdjustedRef.current = true; zoomBy(ZOOM_STEP); }} />
      <div
        ref={(element) => { layerRef.current = element; machineLayerRef.current = element; }}
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
          {gatewayGraph.edges.map((edge) => {
            const machine = layout.find((item) => item.id === edge.machineId);
            if (!machine) return null;
            const paint = meshEdgeModel(geometry.web, { ...machine, width: MACHINE_CARD_WIDTH }, edge.online);
            const label = `${machine.name}: ${edge.online ? "online" : "offline"} ${edge.label}`;
            const launch = () => {
              if (machine.isThisMachine) onWebClick?.();
              else if (machine.webUrl) onOpenPeerWebUi?.(machine.webUrl);
            };
            return (
              <g key={edge.machineId} role="button" tabIndex={0} aria-label={label} aria-disabled={!machine.isThisMachine && !machine.webUrl}
                style={{ pointerEvents: "auto", cursor: "pointer" }}
                onClick={launch}
                onKeyDown={(event) => {
                  if (event.code === "Enter" || event.code === "Space") { event.preventDefault(); launch(); }
                }}>
                <title>{label}</title>
                <path d={paint.path} fill="none" stroke="var(--ct-muted-fg)" strokeWidth={1.2} strokeLinecap="round" opacity={paint.opacity} strokeDasharray={paint.dash} />
              </g>
            );
          })}

        </svg>
        <div ref={webCardRef} data-peer-card className="absolute" style={{ left: gateway.x, top: gateway.y, width: geometry.web.width, transform: "translate(-50%,-50%)" }}>
          <div style={{ zoom: WEB_CARD_SCALE, width: geometry.web.width / WEB_CARD_SCALE }}>
            {webCard ? webCard(geometry.qrSize / WEB_CARD_SCALE) : <button type="button" aria-label="Web" aria-pressed={webSelected} onClick={onWebClick}
              className="w-full rounded-xl border border-[var(--ct-accent)] bg-[var(--ct-bg)] p-4">
              <Globe size={24} aria-hidden="true" /><strong>Web</strong><span>{webStatus}</span>
            </button>}
          </div>
        </div>
        {layout.map((p, i) => (
          <div
            key={`${p.id}-${i}`}
            data-peer-card
            data-machine-index={i}
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
                className="group flex min-h-[88px] w-[280px] items-center gap-3 rounded-lg border border-dashed border-[var(--ct-border)] bg-transparent px-4 text-left text-[var(--ct-muted-fg)] transition-colors hover:border-[color-mix(in_srgb,var(--ct-accent)_65%,var(--ct-border))] hover:bg-[color-mix(in_srgb,var(--ct-accent)_4%,transparent)] hover:text-[var(--ct-fg)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--ct-accent)] cursor-pointer"
                data-peer-card
                aria-label="Add a machine from My machines"
              >
                <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md border border-[var(--ct-border)] bg-[color-mix(in_srgb,var(--ct-fg)_4%,transparent)] text-[var(--ct-muted-fg)] transition-colors group-hover:border-[color-mix(in_srgb,var(--ct-accent)_55%,var(--ct-border))] group-hover:text-[var(--ct-accent)]">
                  <Plus size={16} />
                </span>
                <span className="min-w-0 flex-1">
                  <span className="block text-[12px] font-semibold text-[var(--ct-fg)]">Add a machine</span>
                  <span className="mt-1 block text-[10px] text-[var(--ct-muted-fg)]">Link machines signed in to your account</span>
                </span>
                <ArrowRight size={15} className="shrink-0 text-[var(--ct-muted)] transition-transform group-hover:translate-x-0.5 group-hover:text-[var(--ct-accent)]" />
              </button>
            ) : (
              <div style={{ transform: `scale(${MACHINE_CARD_SCALE})` }}>
              <PeerTerminal
                inventory={peerInventoryModel(services, peers.find((peer) => peer.instance_id === p.id) ?? { instance_id: p.id, name: p.name, online: true }, !!p.isThisMachine, loading, error)}
                name={p.name}
                ip={p.ip}
                isThisMachine={p.isThisMachine}
                isPaired={p.isPaired}
                pending={p.pending}
                accessLevel={p.accessLevel as "full" | "read_only" | "agent_isolated" | undefined}
                onClick={() => onPeerClick(p.name)}
                offline={p.isThisMachine ? !meshEnabled : !peers.find((peer) => peer.instance_id === p.id)?.online}
                onGoOnline={p.isThisMachine ? onGoOnline : undefined}
              />
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
});
