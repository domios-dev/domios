import { useMemo } from "react";
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
  onAddService: () => void;
  width: number;
  height: number;
}

const ACTIVE_WINDOW_MS = 5 * 60 * 1000;
const PULSE_WINDOW_MS = 3000;

export function MeshCanvas({
  thisMachine,
  peers,
  services,
  recentPushes,
  onPeerClick,
  onAddService,
  width,
  height,
}: Props) {
  const layout = useMemo<PeerLayout[]>(() => {
    const cx = width / 2;
    const cy = height / 2;
    const radius = Math.max(120, Math.min(width, height) / 2 - 130);
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

  return (
    <div
      className="relative w-full h-full overflow-hidden"
      style={{
        minHeight: height,
        background:
          "radial-gradient(ellipse at center, var(--ct-titlebar-bg) 0%, var(--ct-bg) 100%)",
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
          const recent = peerPushes.some(
            (rp) => now - rp.at < ACTIVE_WINDOW_MS,
          );
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
              active={recent}
              pulsing={pulsing}
            />
          );
        })}
      </svg>
      {layout.map((p, i) => (
        <div
          key={`${p.name}-${i}`}
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
            onAddService={p.isThisMachine ? onAddService : undefined}
          />
        </div>
      ))}
    </div>
  );
}
