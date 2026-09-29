import { useEffect, useRef } from "react";
import type { SwarmState, SwarmNode, SwarmEdge } from "./types";
import { NODE_W, NODE_H } from "./layout";

// Role icons (simple text)
const ROLE_ICON: Record<string, string> = {
  Orchestrator: "\u2699",  // gear
  Worker: "\u25CF",        // circle
};

/**
 * Resolve a CSS custom property from the document root.
 * Falls back to the provided default if the variable is not set.
 */
function getCSSVar(name: string, fallback: string): string {
  const val = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return val || fallback;
}

/** Parse a hex color to [r, g, b]. */
function hexToRgb(hex: string): [number, number, number] {
  const c = hex.replace("#", "");
  return [
    parseInt(c.substring(0, 2), 16),
    parseInt(c.substring(2, 4), 16),
    parseInt(c.substring(4, 6), 16),
  ];
}

/** Convert [r,g,b] + alpha to rgba string. */
function rgba(hex: string, alpha: number): string {
  const [r, g, b] = hexToRgb(hex);
  return `rgba(${r},${g},${b},${alpha})`;
}

interface ThemeColors {
  bg: string;
  fg: string;
  border: string;
  accent: string;  // blue
  green: string;
  yellow: string;
  red: string;
  muted: string;
  mutedFg: string;
  surface: string;
  cyan: string;
  magenta: string;
}

function readThemeColors(): ThemeColors {
  return {
    bg: getCSSVar("--ct-bg", "#1a1b26"),
    fg: getCSSVar("--ct-fg", "#c0caf5"),
    border: getCSSVar("--ct-border", "#3b4261"),
    accent: getCSSVar("--ct-accent", "#7aa2f7"),
    green: getCSSVar("--ct-green", "#9ece6a"),
    yellow: getCSSVar("--ct-yellow", "#e0af68"),
    red: getCSSVar("--ct-red", "#f7768e"),
    muted: getCSSVar("--ct-muted", "#565f89"),
    mutedFg: getCSSVar("--ct-muted-fg", "#a9b1d6"),
    surface: getCSSVar("--ct-surface", "#24283b"),
    cyan: getCSSVar("--ct-accent", "#7dcfff"),
    magenta: getCSSVar("--ct-red", "#bb9af7"),
  };
}

function getRoleBorderColor(role: string | null, colors: ThemeColors): string {
  switch (role) {
    case "Orchestrator":
      return colors.accent;
    case "Worker":
      return rgba(colors.fg, 0.6);
    default:
      return colors.border;
  }
}

function getStatusColor(status: string, colors: ThemeColors): string {
  switch (status) {
    case "Working": return colors.green;
    case "Waiting": return colors.yellow;
    case "Error": return colors.red;
    case "Dead": return colors.red;
    default: return colors.muted;
  }
}

/** Ease-out cubic. */
function easeOut(t: number): number {
  return 1 - Math.pow(1 - t, 3);
}

/** Spring-like easing for spawn animation. */
function spring(t: number): number {
  if (t >= 1) return 1;
  return 1 - Math.pow(1 - t, 3) * Math.cos(t * Math.PI * 0.5);
}

/** Cubic bezier control points for parent-child edge. */
function edgeBezier(
  parentNode: SwarmNode,
  childNode: SwarmNode,
): { x0: number; y0: number; cx0: number; cy0: number; cx1: number; cy1: number; x1: number; y1: number } {
  const x0 = parentNode.x + NODE_W / 2;
  const y0 = parentNode.y + NODE_H;
  const x1 = childNode.x + NODE_W / 2;
  const y1 = childNode.y;
  const midY = (y0 + y1) / 2;
  return { x0, y0, cx0: x0, cy0: midY, cx1: x1, cy1: midY, x1, y1 };
}

/** Get point on cubic bezier at t in [0,1]. */
function bezierPoint(
  x0: number, y0: number,
  cx0: number, cy0: number,
  cx1: number, cy1: number,
  x1: number, y1: number,
  t: number,
): { x: number; y: number } {
  const mt = 1 - t;
  const mt2 = mt * mt;
  const mt3 = mt2 * mt;
  const t2 = t * t;
  const t3 = t2 * t;
  return {
    x: mt3 * x0 + 3 * mt2 * t * cx0 + 3 * mt * t2 * cx1 + t3 * x1,
    y: mt3 * y0 + 3 * mt2 * t * cy0 + 3 * mt * t2 * cy1 + t3 * y1,
  };
}

export function useSwarmCanvas(
  canvasRef: React.RefObject<HTMLCanvasElement | null>,
  swarmState: SwarmState,
  visible: boolean,
) {
  const rafRef = useRef<number>(0);
  const colorsRef = useRef<ThemeColors>(readThemeColors());

  useEffect(() => {
    if (visible) {
      colorsRef.current = readThemeColors();
    }
  }, [visible]);

  useEffect(() => {
    if (!visible) {
      if (rafRef.current) cancelAnimationFrame(rafRef.current);
      return;
    }

    const canvas = canvasRef.current;
    if (!canvas) return;

    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    function draw() {
      if (!ctx || !canvas) return;
      const now = performance.now();
      const colors = colorsRef.current;
      const dpr = window.devicePixelRatio || 1;

      const w = window.innerWidth;
      const h = window.innerHeight;
      if (canvas.width !== w * dpr || canvas.height !== h * dpr) {
        canvas.width = w * dpr;
        canvas.height = h * dpr;
        canvas.style.width = `${w}px`;
        canvas.style.height = `${h}px`;
        ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      }

      ctx.clearRect(0, 0, w, h);

      const vp = swarmState.viewport;
      ctx.save();
      ctx.translate(vp.x, vp.y);
      ctx.scale(vp.zoom, vp.zoom);

      drawDotGrid(ctx, w, h, vp, colors);

      for (const node of swarmState.nodes.values()) {
        node.x += (node.targetX - node.x) * 0.12;
        node.y += (node.targetY - node.y) * 0.12;
      }

      for (const edge of swarmState.edges.values()) {
        drawEdge(ctx, edge, swarmState, now, colors);
      }

      for (const node of swarmState.nodes.values()) {
        drawNode(ctx, node, now, colors);
      }

      ctx.restore();

      rafRef.current = requestAnimationFrame(draw);
    }

    rafRef.current = requestAnimationFrame(draw);

    return () => {
      if (rafRef.current) cancelAnimationFrame(rafRef.current);
    };
  }, [visible, canvasRef, swarmState]);
}

function drawDotGrid(
  ctx: CanvasRenderingContext2D,
  w: number, h: number,
  vp: { x: number; y: number; zoom: number },
  colors: ThemeColors,
) {
  const spacing = 24;
  ctx.fillStyle = rgba(colors.border, 0.12);

  const startX = Math.floor(-vp.x / (vp.zoom * spacing)) * spacing - spacing;
  const startY = Math.floor(-vp.y / (vp.zoom * spacing)) * spacing - spacing;
  const endX = startX + (w / vp.zoom) + spacing * 2;
  const endY = startY + (h / vp.zoom) + spacing * 2;

  for (let x = startX; x < endX; x += spacing) {
    for (let y = startY; y < endY; y += spacing) {
      ctx.fillRect(x, y, 1, 1);
    }
  }
}

function drawEdge(
  ctx: CanvasRenderingContext2D,
  edge: SwarmEdge,
  state: SwarmState,
  now: number,
  colors: ThemeColors,
) {
  const parentNode = state.nodes.get(edge.from);
  const childNode = state.nodes.get(edge.to);
  if (!parentNode || !childNode) return;

  const childDead = childNode.deadTime !== null;
  let edgeOpacity = 1;
  if (childDead) {
    const elapsed = now - childNode.deadTime!;
    const retractProgress = Math.min(1, elapsed / 300);
    edgeOpacity = 1 - retractProgress;
    if (edgeOpacity <= 0) return;
  }

  const b = edgeBezier(parentNode, childNode);

  let edgeGlow = false;
  for (const p of edge.particles) {
    if (p.progress > 0 && p.progress < 1) {
      edgeGlow = true;
      break;
    }
  }

  ctx.beginPath();
  ctx.moveTo(b.x0, b.y0);
  ctx.bezierCurveTo(b.cx0, b.cy0, b.cx1, b.cy1, b.x1, b.y1);
  ctx.strokeStyle = edgeGlow
    ? rgba(colors.green, 0.7 * edgeOpacity)
    : rgba(colors.border, 0.5 * edgeOpacity);
  ctx.lineWidth = 1.5;
  ctx.stroke();

  // Draw particles (travel from child to parent)
  for (let i = edge.particles.length - 1; i >= 0; i--) {
    const p = edge.particles[i];
    const elapsed = now - p.startTime;
    const duration = 600;
    p.progress = Math.min(1, easeOut(elapsed / duration));

    if (p.progress >= 1) {
      parentNode.flashColor = colors.green;
      parentNode.flashStart = now;
      edge.particles.splice(i, 1);
      continue;
    }

    // Particle travels from child (t=0) to parent (t=1). Progress maps child at bezier t=1 and parent at t=0.
    const t = 1 - p.progress;
    const pt = bezierPoint(b.x0, b.y0, b.cx0, b.cy0, b.cx1, b.cy1, b.x1, b.y1, t);

    ctx.beginPath();
    ctx.arc(pt.x, pt.y, 12, 0, Math.PI * 2);
    const gradient = ctx.createRadialGradient(pt.x, pt.y, 0, pt.x, pt.y, 12);
    gradient.addColorStop(0, rgba(colors.green, 0.6));
    gradient.addColorStop(1, rgba(colors.green, 0));
    ctx.fillStyle = gradient;
    ctx.fill();

    ctx.beginPath();
    ctx.arc(pt.x, pt.y, 6, 0, Math.PI * 2);
    ctx.fillStyle = colors.green;
    ctx.fill();
  }
}

function drawNode(
  ctx: CanvasRenderingContext2D,
  node: SwarmNode,
  now: number,
  colors: ThemeColors,
) {
  const roleBorder = getRoleBorderColor(node.role, colors);
  const statusColor = getStatusColor(node.status, colors);
  const icon = ROLE_ICON[node.role || ""] || "\u25CB";

  const spawnElapsed = now - node.spawnTime;
  const spawnDuration = 400;
  const spawnProgress = Math.min(1, spawnElapsed / spawnDuration);
  const scale = spring(spawnProgress);
  const opacity = Math.min(1, spawnProgress * 2); // fade in faster than scale

  let deathScale = 1;
  let deathOpacity = 1;
  if (node.deadTime !== null) {
    const deathElapsed = now - node.deadTime;
    deathScale = Math.max(0.7, 1 - (deathElapsed / 200) * 0.3);
    deathOpacity = Math.max(0, 1 - deathElapsed / 400);
    if (deathOpacity <= 0) return;
  }

  const finalScale = scale * deathScale;
  const finalOpacity = opacity * deathOpacity;

  const cx = node.x + NODE_W / 2;
  const cy = node.y + NODE_H / 2;

  ctx.save();
  ctx.translate(cx, cy);
  ctx.scale(finalScale, finalScale);
  ctx.translate(-cx, -cy);
  ctx.globalAlpha = finalOpacity;

  if (node.status === "Working" && node.deadTime === null) {
    const pulse = (Math.sin(now / 800 * Math.PI) + 1) / 2; // 0 to 1, 1.6s cycle
    const glowSize = pulse * 4;
    ctx.shadowColor = rgba(roleBorder, 0.5);
    ctx.shadowBlur = glowSize;
  }

  // Flash effect (on report arrival)
  let currentBorderColor = roleBorder;
  if (node.flashColor && now - node.flashStart < 300) {
    const flashProgress = (now - node.flashStart) / 300;
    currentBorderColor = flashProgress < 0.5 ? node.flashColor : roleBorder;
  } else {
    node.flashColor = null;
  }

  const r = 6; // border radius
  ctx.beginPath();
  roundRect(ctx, node.x, node.y, NODE_W, NODE_H, r);
  ctx.fillStyle = rgba(colors.surface, 0.95);
  ctx.fill();
  ctx.strokeStyle = currentBorderColor;
  ctx.lineWidth = 1.5;
  ctx.stroke();

  ctx.shadowColor = "transparent";
  ctx.shadowBlur = 0;

  ctx.fillStyle = colors.fg;
  ctx.font = "11px -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif";
  ctx.textBaseline = "middle";
  const titleText = `${icon} ${truncate(node.title, 14)}`;
  ctx.fillText(titleText, node.x + 8, node.y + 20);

  ctx.fillStyle = statusColor;
  ctx.font = "9px -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif";
  ctx.fillText(node.status, node.x + 8, node.y + 38);

  ctx.globalAlpha = 1;
  ctx.restore();

  if (spawnElapsed < 500 && spawnElapsed > 0) {
    const burstProgress = spawnElapsed / 500;
    const burstScale = 1 + burstProgress;
    const burstOpacity = 1 - burstProgress;

    ctx.save();
    ctx.globalAlpha = burstOpacity * 0.6;
    ctx.strokeStyle = roleBorder;
    ctx.lineWidth = 2;
    ctx.translate(cx, cy);
    ctx.scale(burstScale, burstScale);
    ctx.beginPath();
    roundRect(ctx, -NODE_W / 2, -NODE_H / 2, NODE_W, NODE_H, r);
    ctx.stroke();
    ctx.restore();
  }
}

function roundRect(
  ctx: CanvasRenderingContext2D,
  x: number, y: number,
  w: number, h: number,
  radius: number,
) {
  ctx.moveTo(x + radius, y);
  ctx.lineTo(x + w - radius, y);
  ctx.arcTo(x + w, y, x + w, y + radius, radius);
  ctx.lineTo(x + w, y + h - radius);
  ctx.arcTo(x + w, y + h, x + w - radius, y + h, radius);
  ctx.lineTo(x + radius, y + h);
  ctx.arcTo(x, y + h, x, y + h - radius, radius);
  ctx.lineTo(x, y + radius);
  ctx.arcTo(x, y, x + radius, y, radius);
  ctx.closePath();
}

function truncate(str: string, max: number): string {
  if (str.length <= max) return str;
  return str.slice(0, max - 1) + "\u2026";
}
