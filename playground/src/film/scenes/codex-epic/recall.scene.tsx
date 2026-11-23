import { useLayoutEffect, useRef } from "react";
import { readMemoryGraphPalette } from "../mesh/holoPalette";
import topology from "../../data/memory-topology.json";
import { range, useStage, type Scene } from "../../engine";
import { accent, Frame, muted, Rule, Slam, Slug } from "./craft";

function Constellation({ t, portrait }: { t: number; portrait: boolean }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const width = portrait ? 930 : 1150;
  const height = portrait ? 800 : 1000;
  useLayoutEffect(() => {
    const ctx = ref.current?.getContext("2d");
    if (!ctx) return;
    const palette = readMemoryGraphPalette();
    const zoom = 1.05 - .55 * range(t, 0, 7.8);
    const turn = .12 * range(t, 0, 8);
    const points = topology.nodes.map(([x, y]) => {
      const px = portrait ? -y : x;
      const py = portrait ? x : y;
      return [width / 2 + (px * Math.cos(turn) - py * Math.sin(turn)) * zoom, height / 2 + (px * Math.sin(turn) + py * Math.cos(turn)) * zoom];
    });
    ctx.clearRect(0, 0, width, height);
    ctx.strokeStyle = `rgba(${palette.edgeRgb},0.11)`;
    ctx.lineWidth = .65;
    ctx.beginPath();
    for (const [a, b] of topology.edges) {
      ctx.moveTo(points[a][0], points[a][1]);
      ctx.lineTo(points[b][0], points[b][1]);
    }
    ctx.stroke();
    const style = getComputedStyle(document.documentElement);
    for (const entity of [true, false]) {
      ctx.fillStyle = entity ? style.getPropertyValue("--ct-muted-fg").trim() : style.getPropertyValue("--ct-accent").trim();
      ctx.beginPath();
      topology.nodes.forEach(([, , kind, degree], i) => {
        if ((kind < 0) !== entity) return;
        const r = (entity ? 1.2 : 2.1) + Math.sqrt(degree) * .12;
        ctx.moveTo(points[i][0] + r, points[i][1]);
        ctx.arc(points[i][0], points[i][1], r, 0, Math.PI * 2);
      });
      ctx.fill();
    }
  }, [t, portrait, width, height]);
  return <canvas ref={ref} width={width} height={height} style={{ position: "absolute", left: portrait ? 75 : 740, top: portrait ? 590 : 10, width, height }} />;
}

function Recall({ t }: { t: number }) {
  const { portrait } = useStage();
  return <Frame>
    <Constellation t={t} portrait={portrait} />
    <Slug style={{ left: portrait ? 115 : 80, top: portrait ? 100 : 72 }}>Domios / memory at scale</Slug>
    <div style={{ position: "absolute", left: portrait ? 115 : 70, top: portrait ? 220 : 260 }}>
      <Slam t={t} at={0} size={portrait ? 235 : 250}>{topology.totals.nodes.toLocaleString("en-US")}</Slam>
      <Slam t={t} at={1.05} size={portrait ? 106 : 113} style={{ marginTop: 28 }}>POINTS OF</Slam>
      <Slam t={t} at={1.48} size={portrait ? 137 : 147} style={{ color: accent, marginTop: 12 }}>RECALL.</Slam>
    </div>
    <Rule t={t} at={2.2} left={portrait ? 115 : 80} top={portrait ? 1360 : 865} width={portrait ? 850 : 570} />
    <div style={{ position: "absolute", left: portrait ? 115 : 80, top: portrait ? 1390 : 898, fontSize: 30, color: muted }}>
      {topology.totals.edges.toLocaleString("en-US")} relations.
    </div>
    <Slug style={{ left: portrait ? 115 : 80, top: portrait ? 1440 : 1002, fontSize: 26 }}>mac-mini · real memory counts</Slug>
  </Frame>;
}

export default { id: "cx-epic-recall", duration: 8, render: (t) => <Recall t={t} /> } satisfies Scene;
