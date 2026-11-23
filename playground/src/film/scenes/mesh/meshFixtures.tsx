import type { CSSProperties, ReactNode } from "react";
import { PeerTerminal } from "../../../../../src/components/MeshPanel/PeerTerminal";
import { peerInventoryModel } from "../../../../../src/components/MeshPanel/peerInventoryModel";
import type { Service } from "../../../../../src/components/MeshPanel/types";
import type { PeerStatus } from "../../../../../src/components/MeshPanel/useMeshData";
import { clamp, easeOut, lerp, range } from "../../engine";

export const W = 1920;
export const H = 1080;

/** The 9:16 frame; key content stays left of SAFE_X and above SAFE_Y, clear of the Reels UI. */
export const PW = 1080;
export const PH = 1920;
export const SAFE_X = 960;
export const SAFE_Y = 1536;

export type PeerName = "mac-mini" | "homelab" | "studio" | "gpu-rig" | "pi-hub" | "laptop";

export type Resource = [name: string, kind: "daemon" | "cron", state?: "running" | "degraded"];

const MACHINES: Record<PeerName, { ip: string; resources: Resource[] }> = {
  "mac-mini": { ip: "100.84.12.7", resources: [["gateway", "daemon"], ["home-assistant", "daemon"], ["relay", "daemon"], ["nightly-backup", "cron"], ["mem-dreamer", "cron"]] },
  homelab: { ip: "100.101.44.18", resources: [["postgres", "daemon"], ["ollama", "daemon"], ["grafana", "daemon"], ["apt-upgrade", "cron"]] },
  studio: { ip: "100.92.7.61", resources: [["transcriber", "daemon"], ["whisper-api", "daemon"], ["inbox-digest", "cron"]] },
  "gpu-rig": { ip: "100.77.150.3", resources: [["comfyui", "daemon"], ["llama-server", "daemon"], ["jellyfin", "daemon"], ["render-queue", "cron", "running"]] },
  "pi-hub": { ip: "100.66.21.40", resources: [["mosquitto", "daemon"], ["zigbee2mqtt", "daemon"], ["plant-sensors", "cron"]] },
  laptop: { ip: "100.110.9.24", resources: [["vite", "daemon"], ["playwright", "daemon", "degraded"], ["standup-notes", "cron"]] },
};

export const PEER_NAMES = Object.keys(MACHINES) as PeerName[];

const REGISTERED_AT = 1_790_000_000;

function service(peer: PeerName, [name, kind, state]: Resource): Service {
  return {
    id: `${peer}-${name}`,
    name,
    kind,
    health_mode: "process",
    health_interval: 30,
    health: { status: state === "degraded" ? "degraded" : "healthy" },
    push_enabled: true,
    registered_at: REGISTERED_AT,
    updated_at: REGISTERED_AT,
    enabled: true,
    last_run_status: kind === "cron" ? (state === "running" ? "running" : "ok") : undefined,
    schedule: kind === "cron" ? "0 3 * * *" : undefined,
    source_peer_id: peer,
    source_peer: peer,
  };
}

function peerStatus(peer: PeerName): PeerStatus {
  return { instance_id: peer, name: peer, ip: MACHINES[peer].ip, api_port: 7685, online: true, access_level: "full", last_seen: null, services_loaded: true, relay_protocol_version: 1 };
}

const SERVICES = PEER_NAMES.flatMap((peer) => MACHINES[peer].resources.map((r) => service(peer, r)));

export function peerIp(peer: PeerName) {
  return MACHINES[peer].ip;
}

const NAME_CSS = `.film-card [role="button"] > span.font-medium { font-size: var(--film-name-size); }`;

/** The shipped mesh machine card for one of the owner's peers; `nameSize` enlarges only the machine name for film legibility. */
/** `extra` adds services the scene installs on the machine as it plays. */
export function MachineCard({ peer, self = false, scale = 1, nameSize, glow = 0, extra = [] }: { peer: PeerName; self?: boolean; scale?: number; nameSize?: number; glow?: number; extra?: Resource[] }) {
  const lit = glow > 0 ? { borderRadius: 6, boxShadow: `0 0 0 1.5px color-mix(in srgb, var(--ct-accent) ${Math.round(glow * 100)}%, transparent), 0 0 ${Math.round(28 * glow)}px color-mix(in srgb, var(--ct-accent) ${Math.round(glow * 50)}%, transparent)` } : undefined;
  return (
    <div className={nameSize ? "film-card" : undefined} data-film-hint={`${peer}: ${self ? "this machine" : "a paired machine"} in your private mesh`} style={{ transform: `scale(${scale})`, transformOrigin: "50% 50%", ...(nameSize ? { ["--film-name-size" as string]: `${nameSize}px` } : undefined), ...lit }}>
      {nameSize ? <style>{NAME_CSS}</style> : null}
      <PeerTerminal
        inventory={peerInventoryModel(extra.length ? [...SERVICES, ...extra.map((r) => service(peer, r))] : SERVICES, peerStatus(peer))}
        name={peer}
        ip={MACHINES[peer].ip}
        isThisMachine={self}
        isPaired={!self}
        accessLevel={self ? undefined : "full"}
      />
    </div>
  );
}

/** Absolutely centres children on a world point. */
export function At({ x, y, children, z, style }: { x: number; y: number; children: ReactNode; z?: number; style?: CSSProperties }) {
  return <div style={{ position: "absolute", left: x, top: y, transform: "translate(-50%, -50%)", zIndex: z, ...style }}>{children}</div>;
}

/** Fade and settle a card in from a slight scale-down at `at`. */
export function Appear({ t, at, dur = 0.6, children }: { t: number; at: number; dur?: number; children: ReactNode }) {
  const k = range(t, at, at + dur, easeOut);
  return <div style={{ opacity: k, transform: `scale(${lerp(0.94, 1, k)})` }}>{children}</div>;
}

export const meshBackground = "radial-gradient(ellipse at center, var(--ct-titlebar-bg) 0%, var(--ct-bg) 100%)";

/** A glowing accent pulse travelling from a to b over [start, end]; hidden outside that window. */
export function Pulse({ t, from, to, start, end, r = 7 }: { t: number; from: [number, number]; to: [number, number]; start: number; end: number; r?: number }) {
  if (t < start || t > end) return null;
  const k = range(t, start, end);
  const trail = clamp(k - 0.12, 0, 1);
  const x = lerp(from[0], to[0], k);
  const y = lerp(from[1], to[1], k);
  return (
    <g>
      <line x1={lerp(from[0], to[0], trail)} y1={lerp(from[1], to[1], trail)} x2={x} y2={y} stroke="var(--ct-accent)" strokeWidth={r * 0.7} strokeLinecap="round" opacity={0.55} />
      <circle cx={x} cy={y} r={r * 2.6} fill="var(--ct-accent)" opacity={0.18} />
      <circle cx={x} cy={y} r={r} fill="var(--ct-accent)" />
    </g>
  );
}

/** An expanding accent ring marking an arrival at `at`. */
export function Ripple({ t, x, y, at, size = 60 }: { t: number; x: number; y: number; at: number; size?: number }) {
  if (t < at || t > at + 0.9) return null;
  const k = range(t, at, at + 0.9, easeOut);
  return <circle cx={x} cy={y} r={8 + size * k} fill="none" stroke="var(--ct-accent)" strokeWidth={2.5} opacity={0.8 * (1 - k)} />;
}

export function Caption({ children, style }: { children: ReactNode; style?: CSSProperties }) {
  return <div style={{ fontSize: 22, letterSpacing: "0.01em", color: "var(--ct-muted-fg)", ...style }}>{children}</div>;
}

export function Typed({ t, at, cps = 40, text }: { t: number; at: number; cps?: number; text: string }) {
  const n = Math.max(0, Math.min(text.length, Math.floor((t - at) * cps)));
  const caret = t >= at && n < text.length;
  return (
    <span>
      {text.slice(0, n)}
      <span style={{ opacity: caret ? 1 : 0, color: "var(--ct-accent)" }}>▍</span>
    </span>
  );
}
