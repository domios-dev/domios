import { useRef, useEffect, useCallback, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { listen } from "../../lib/bridge";
import { useMultiplexer } from "../../store/multiplexer";
import { useSwarmCanvas } from "./useSwarmCanvas";
import { computeTreeLayout } from "./layout";
import type { SwarmState, SwarmNode, SwarmEdge, ReportParticle } from "./types";
import type { Pane, PaneStatus } from "../../lib/types";

let particleCounter = 0;

function mapRoleToSwarm(role: Pane["role"]): SwarmNode["role"] {
  switch (role) {
    case "Orchestrator":
      return "Orchestrator";
    case "Worker":
      return "Worker";
    default:
      return null;
  }
}

function mapPaneStatus(
  paneId: string,
  paneStatuses: Record<string, PaneStatus>,
): SwarmNode["status"] {
  const status = paneStatuses[paneId];
  if (!status) return "Idle";
  return status;
}

interface SwarmOverlayProps {
  visible: boolean;
  onClose: () => void;
}

export function SwarmOverlay({ visible, onClose }: SwarmOverlayProps) {
  const { state } = useMultiplexer();
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [animState, setAnimState] = useState<"entering" | "visible" | "exiting" | "hidden">(
    visible ? "entering" : "hidden"
  );

  // Track previous visibility for transitions
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

  // Build swarm state from multiplexer panes
  const swarmState = useMemo<SwarmState>(() => {
    const nodes = new Map<string, SwarmNode>();
    const edges = new Map<string, SwarmEdge>();

    // Filter panes that have an agent role
    for (const pane of Object.values(state.panes)) {
      if (!pane.role) continue;

      const mappedRole = mapRoleToSwarm(pane.role);
      const mappedStatus = mapPaneStatus(pane.id, (state as any).paneStatuses || {});

      nodes.set(pane.id, {
        id: pane.id,
        title: pane.title || pane.id.slice(0, 8),
        role: mappedRole,
        status: mappedStatus,
        parentId: pane.parent_pane_id || null,
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

    // Build edges from parent-child relationships
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
  }, [state.panes, (state as any).paneStatuses]); // eslint-disable-line react-hooks/exhaustive-deps

  // Stable ref for swarm state so rAF loop and event handlers share same object
  const swarmRef = useRef(swarmState);
  swarmRef.current = swarmState;

  // Recompute layout whenever nodes change
  useEffect(() => {
    computeTreeLayout(
      swarmState.nodes,
      window.innerWidth,
      window.innerHeight,
    );
  }, [swarmState.nodes]);

  // Listen for agent-report events to spawn particles
  useEffect(() => {
    if (!visible) return;

    let cancelled = false;

    // Listen for pane status changes
    const unsubs: (() => void)[] = [];

    listen<{ pane_id: string; parent_pane_id?: string }>("agent-report", (event) => {
      if (cancelled) return;
      const { pane_id, parent_pane_id } = event.payload;

      // Find the edge from this child to its parent
      const sw = swarmRef.current;
      const childNode = sw.nodes.get(pane_id);
      if (!childNode) return;

      const parentId = parent_pane_id || childNode.parentId;
      if (!parentId) return;

      const edgeKey = `${parentId}->${pane_id}`;
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

  // Close on Escape
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

  // Canvas rendering hook
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
