import { useRef, useEffect, useCallback, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { listen } from "../../lib/bridge";
import { useMuxState } from "../../store/multiplexer";
import { useSwarmCanvas } from "./useSwarmCanvas";
import { computeTreeLayout } from "./layout";
import type { SwarmState, SwarmNode, SwarmEdge, ReportParticle } from "./types";
import type { Tab, TabStatus } from "../../lib/types";
import { ORCHESTRATOR_KIND, roleKind } from "../../lib/roleKind";

let particleCounter = 0;

function mapRoleToSwarm(tab: Pick<Tab, "role" | "role_profile">): SwarmNode["role"] {
  if (tab.role === "Worker") return "Worker";
  return roleKind(tab) === ORCHESTRATOR_KIND ? "Orchestrator" : null;
}

function mapTabStatus(
  tabId: string,
  tabStatuses: Record<string, TabStatus>,
): SwarmNode["status"] {
  const status = tabStatuses[tabId];
  if (!status) return "Idle";
  return status;
}

interface SwarmOverlayProps {
  visible: boolean;
  onClose: () => void;
}

export function SwarmOverlay({ visible, onClose }: SwarmOverlayProps) {
  const state = useMuxState();
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [animState, setAnimState] = useState<"entering" | "visible" | "exiting" | "hidden">(
    visible ? "entering" : "hidden"
  );

  useEffect(() => {
    if (visible) {
      setAnimState("entering");
      const t = setTimeout(() => setAnimState("visible"), 200);
      return () => clearTimeout(t);
    } else if (animState !== "hidden") {
      setAnimState("exiting");
      const t = setTimeout(() => setAnimState("hidden"), 120);
      return () => clearTimeout(t);
    }
  }, [visible]); // eslint-disable-line react-hooks/exhaustive-deps

  const swarmState = useMemo<SwarmState>(() => {
    const nodes = new Map<string, SwarmNode>();
    const edges = new Map<string, SwarmEdge>();

    for (const tab of Object.values(state.tabs)) {
      if (!tab.role) continue;

      const mappedRole = mapRoleToSwarm(tab);
      const mappedStatus = mapTabStatus(tab.id, (state as any).tabStatuses || {});

      nodes.set(tab.id, {
        id: tab.id,
        title: tab.title || tab.id.slice(0, 8),
        role: mappedRole,
        status: mappedStatus,
        parentId: tab.parent_tab_id || null,
        x: 0,
        y: 0,
        targetX: 0,
        targetY: 0,
        spawnTime: performance.now(),
        deadTime: mappedStatus === "Dead" ? performance.now() : null,
        flashColor: null,
        flashStart: 0,
      });
    }

    for (const node of nodes.values()) {
      if (node.parentId && nodes.has(node.parentId)) {
        const edgeKey = `${node.parentId}->${node.id}`;
        edges.set(edgeKey, {
          from: node.parentId,
          to: node.id,
          particles: [],
        });
      }
    }

    return { nodes, edges, viewport: { x: 0, y: 0, zoom: 1 } };
  }, [state.tabs, (state as any).tabStatuses]); // eslint-disable-line react-hooks/exhaustive-deps

  const swarmRef = useRef(swarmState);
  swarmRef.current = swarmState;

  useEffect(() => {
    computeTreeLayout(
      swarmState.nodes,
      window.innerWidth,
      window.innerHeight,
    );
  }, [swarmState.nodes]);

  useEffect(() => {
    if (!visible) return;

    let cancelled = false;

    const unsubs: (() => void)[] = [];

    listen<{ tab_id: string; parent_tab_id?: string }>("agent-report", (event) => {
      if (cancelled) return;
      const { tab_id, parent_tab_id } = event.payload;

      const sw = swarmRef.current;
      const childNode = sw.nodes.get(tab_id);
      if (!childNode) return;

      const parentId = parent_tab_id || childNode.parentId;
      if (!parentId) return;

      const edgeKey = `${parentId}->${tab_id}`;
      const edge = sw.edges.get(edgeKey);
      if (!edge) return;

      const particle: ReportParticle = {
        id: `p-${++particleCounter}`,
        progress: 0,
        startTime: performance.now(),
      };
      edge.particles.push(particle);
    }).then((u) => { if (!cancelled) unsubs.push(u); });

    return () => {
      cancelled = true;
      for (const u of unsubs) u();
    };
  }, [visible]);

  useEffect(() => {
    if (!visible) return;
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", handleKey, { capture: true });
    return () => window.removeEventListener("keydown", handleKey, { capture: true });
  }, [visible, onClose]);

  useSwarmCanvas(canvasRef, swarmState, visible && animState !== "hidden");

  const handleBackdropClick = useCallback((e: React.MouseEvent) => {
    if (e.target === e.currentTarget) onClose();
  }, [onClose]);

  if (animState === "hidden") return null;

  const overlayOpacity = animState === "entering" ? 0 : animState === "exiting" ? 0 : 1;
  const transition = animState === "entering"
    ? "opacity 200ms ease-out"
    : "opacity 120ms ease-in";

  return createPortal(
    <div
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 99999,
        opacity: overlayOpacity,
        transition,
        pointerEvents: "auto",
      }}
      onClick={handleBackdropClick}
    >
      {/* Backdrop */}
      <div
        style={{
          position: "absolute",
          inset: 0,
          backgroundColor: "rgba(0, 0, 0, 0.85)",
          backdropFilter: "blur(24px)",
          WebkitBackdropFilter: "blur(24px)",
        }}
      />

      {/* Canvas */}
      <canvas
        ref={canvasRef}
        style={{
          position: "absolute",
          inset: 0,
          width: "100%",
          height: "100%",
        }}
      />

      {/* Header */}
      <div
        style={{
          position: "absolute",
          top: 16,
          left: 24,
          color: "var(--ct-fg, #c0caf5)",
          fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
          fontSize: 13,
          fontWeight: 600,
          opacity: 0.7,
          userSelect: "none",
        }}
      >
        Agent Swarm
        <span style={{ fontWeight: 400, opacity: 0.6, marginLeft: 8 }}>
          {swarmState.nodes.size} agent{swarmState.nodes.size !== 1 ? "s" : ""}
        </span>
      </div>

      {/* Close button */}
      <button
        onClick={onClose}
        style={{
          position: "absolute",
          top: 12,
          right: 16,
          background: "transparent",
          border: "1px solid var(--ct-border, #3b4261)",
          borderRadius: 6,
          color: "var(--ct-fg, #c0caf5)",
          fontSize: 12,
          padding: "4px 12px",
          cursor: "pointer",
          fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
          opacity: 0.7,
          transition: "opacity 150ms",
        }}
        onMouseEnter={(e) => { (e.target as HTMLElement).style.opacity = "1"; }}
        onMouseLeave={(e) => { (e.target as HTMLElement).style.opacity = "0.7"; }}
      >
        ESC
      </button>

      {/* Empty state */}
      {swarmState.nodes.size === 0 && (
        <div
          style={{
            position: "absolute",
            inset: 0,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            flexDirection: "column",
            gap: 8,
            color: "var(--ct-muted-fg, #a9b1d6)",
            fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
            userSelect: "none",
          }}
        >
          <div style={{ fontSize: 14, opacity: 0.6 }}>No active agents</div>
          <div style={{ fontSize: 11, opacity: 0.4 }}>
            Spawn agents with <code style={{ opacity: 0.8 }}>codeterm agent spawn</code>
          </div>
        </div>
      )}
    </div>,
    document.body,
  );
}
