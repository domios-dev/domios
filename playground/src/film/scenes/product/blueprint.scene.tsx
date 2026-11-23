import { CodeBlock } from "@codeterm/chat-ui";
import { BLUEPRINT_RUN, SERVICE_HEALTH_YAML, serviceHealthAt, signalAt } from "../../fixtures/blueprint";
import { BlueprintCanvas } from "../../../../../src/components/Automation/blueprint/Canvas";
import { layoutBlueprint } from "../../../../../src/components/Automation/blueprint/layout";
import { litWires } from "../../../../../src/components/Automation/blueprint/graphModel";
import { createAutomationCamera } from "../../../../../src/components/Automation/blueprint/viewport";
import "../../../../../src/components/Automation/blueprint.css";
import type { CSSProperties } from "react";
import { createPortal } from "react-dom";
import { useBreakout } from "../../breakout";
import { Camera, range, easeOut, track, useStage, type Scene } from "../../engine";

const CUT = 2;
const ORDER = ["$trigger", "health", "diagnose", "$result"];
const ARRIVALS = [2.25, BLUEPRINT_RUN.checkStarts, BLUEPRINT_RUN.diagnoseStarts, BLUEPRINT_RUN.resultDelivering];

function reaction(t: number, at: number | undefined) {
  if (at === undefined) return 0;
  return range(t, at - 0.06, at + 0.08) * (1 - range(t, at + 0.14, at + 0.65));
}

function BlueprintFile({ t }: { t: number }) {
  const { portrait } = useStage();
  const width = portrait ? 460 : 700;
  return <Camera t={t} width={width} height={700} keys={[
    { t: 0, x: width / 2, y: portrait ? 410 : 350, scale: portrait ? 1.95 : 1.6 },
    { t: CUT, x: width / 2, y: portrait ? 414 : 355, scale: portrait ? 1.97 : 1.64 },
  ]}>
    <div style={{ width, position: "absolute", top: 100 }}>
      <p className="ct-meta-muted">service-health.yaml</p>
      <CodeBlock text={SERVICE_HEALTH_YAML} language="yaml" maxHeight="none" wrapLines={portrait} />
    </div>
  </Camera>;
}

/** The pipeline is bigger than the screen: a second copy past the frame shows the rest of it fading into the page. */
function BlueprintGraph({ t }: { t: number }) {
  const { portrait, width: stageWidth, height: stageHeight } = useStage();
  const breakout = useBreakout();
  const graph = portrait
    ? <BlueprintView t={t} left={90} top={340} width={900} height={840} />
    : <BlueprintView t={t} left={0} top={0} width={stageWidth} height={stageHeight} />;
  if (!breakout) return graph;
  const { x: mx, y: my } = breakout.margin;
  const wide = stageWidth + 2 * mx;
  const tall = stageHeight + 2 * my;
  const fade = (dir: string, side: number, length: number) => `linear-gradient(${dir}, transparent 0, #000 ${side}px, #000 ${length - side}px, transparent ${length}px)`;
  const mask: CSSProperties = {
    maskImage: `linear-gradient(#000, #000), ${fade("to right", mx, wide)}, ${fade("to bottom", my, tall)}`,
    maskSize: `${stageWidth}px ${stageHeight}px, 100% 100%, 100% 100%`,
    maskPosition: `${mx}px ${my}px, 0 0, 0 0`,
    maskRepeat: "no-repeat",
    maskComposite: "exclude, intersect",
  };
  return <>
    {graph}
    {createPortal(<div className="film-blueprint-spill" aria-hidden="true" style={{ position: "absolute", left: -mx, top: -my, width: wide, height: tall, ...mask }}>
      <BlueprintView t={t} left={0} top={0} width={wide} height={tall} spill />
    </div>, breakout.node)}
  </>;
}

function BlueprintView({ t, left, top, width, height, spill = false }: { t: number; left: number; top: number; width: number; height: number; spill?: boolean }) {
  const { portrait } = useStage();
  const model = serviceHealthAt(t);
  const layout = layoutBlueprint(model);
  const scale = portrait ? 2.25 : 2.3;
  const legendHeight = 31.25;
  const canvasHeight = height - legendHeight;
  const centers = ORDER.map((id) => {
    const entry = layout.nodes.find((laid) => laid.node.id === id)!;
    return { x: entry.box.x + entry.box.width / 2, y: entry.box.y + entry.box.height / 2 };
  });
  const focus = track(t, [
    { t: CUT, x: centers[0].x }, { t: 3.25, x: centers[1].x },
    { t: 4.2, x: centers[1].x }, { t: 5.45, x: centers[2].x },
    { t: 6.35, x: centers[2].x }, { t: 7.35, x: centers[3].x },
    { t: 8, x: centers[3].x },
  ]);
  // Centred on every node, side branches included, so nothing hangs under the captions.
  const rowY = portrait
    ? centers.reduce((sum, c) => sum + c.y, 0) / centers.length
    : (Math.min(...layout.nodes.map((laid) => laid.box.y)) + Math.max(...layout.nodes.map((laid) => laid.box.y + laid.box.height))) / 2;
  const transform = { scale, x: width / 2 - focus.x * scale, y: canvasHeight / 2 - rowY * scale };
  const camera = { ...createAutomationCamera(), transform, userMoved: true, size: { width, height: canvasHeight } };
  const signal = signalAt(t);
  const route = signal && layout.wires.find((entry) => entry.wire.kind === "exec"
    && model.nodes.find((node) => node.key === entry.wire.from.node)?.id === signal.from
    && model.nodes.find((node) => node.key === entry.wire.to.node)?.id === signal.to
    && (signal.from !== "health" || entry.wire.outcome === "down")
    && (signal.from !== "diagnose" || entry.wire.outcome === "done"));
  const branch = reaction(t, BLUEPRINT_RUN.checkDown);
  const styles = layout.nodes.map(({ node }) => {
    const order = ORDER.indexOf(node.id);
    const index = order === -1 ? 2 : order;
    const build = range(t, CUT + index * 0.18, CUT + index * 0.18 + 0.25, easeOut);
    const glow = Math.max(reaction(t, ARRIVALS[order]), node.id === "health" ? branch : 0,
      node.id === "diagnose" ? reaction(t, BLUEPRINT_RUN.diagnoseDone) : 0,
      node.id === "$result" ? range(t, BLUEPRINT_RUN.resultDelivering, BLUEPRINT_RUN.resultDelivered) * 0.8 : 0);
    return `
      .film-blueprint [data-node-key=${JSON.stringify(node.key)}] {
        opacity: ${build * (node.evidence?.state === "pending" ? 0.62 : 1) * (order === -1 ? 1 - range(t, BLUEPRINT_RUN.checkDown, BLUEPRINT_RUN.checkDown + 0.35) * 0.6 : 1)};
        transform: translateY(${(1 - build) * 10}px) scale(${0.96 + 0.04 * build});
        transform-origin: center;
        border-color: color-mix(in srgb, var(--ct-accent) ${12 + glow * 72}%, var(--bp-line));
        box-shadow: 0 0 0 ${glow * 3}px color-mix(in srgb, var(--ct-accent) ${glow * 20}%, transparent),
          0 0 ${glow * 24}px color-mix(in srgb, var(--ct-accent) ${glow * 25}%, transparent), var(--ct-shadow-sm);
      }
      .film-blueprint [data-node-key=${JSON.stringify(node.key)}] .bp-node__dot {
        background: var(--ct-accent); box-shadow: 0 0 ${glow * 10}px var(--ct-accent);
        opacity: ${node.evidence?.state === "pending" ? 0.4 : 1};
      }`;
  }).join("") + layout.nodes.flatMap((laid) => laid.rows.map((row, index) => {
    if (laid.node.id !== "health" || row.right?.label !== "up") return "";
    return `.film-blueprint [data-node-key=${JSON.stringify(laid.node.key)}] .bp-row:nth-last-child(${laid.rows.length - index}) [data-side="out"] { opacity: ${1 - range(t, BLUEPRINT_RUN.checkDown, BLUEPRINT_RUN.checkDown + 0.35) * 0.8}; }`;
  })).join("");
  return <div className="film-blueprint" data-film-hint={spill ? undefined : "Blueprint: agents, checks and schedules wired into one pipeline"} style={{ position: "absolute", left, top, width, height, display: "flex", flexDirection: "column" }}>
    {spill ? <style>{".film-blueprint-spill .bp-legend, .film-blueprint-spill :has(> [data-testid=canvas-zoom-fit]) { visibility: hidden; }"}</style> : <style>{`
      .film-blueprint .bp-scope { --bp-source: var(--ct-accent); --bp-agent: var(--ct-accent); --bp-result: var(--ct-accent); }
      .film-blueprint .bp-legend { visibility: hidden; }
      .film-blueprint .bp-node, .film-blueprint .bp-wire { animation: none !important; transition: none !important; }
      .film-blueprint .bp-wire { visibility: hidden; }
      ${styles}
    `}</style>}
    <BlueprintCanvas layout={layout} label="Service health pipeline" selected={null} camera={camera} onSelect={() => {}} lit={litWires(model)} />
    <svg aria-hidden="true" width={width} height={canvasHeight} style={{ position: "absolute", top: legendHeight + 1, left: 0, pointerEvents: "none", overflow: "hidden" }}>
      <g transform={`translate(${transform.x} ${transform.y}) scale(${scale})`}>
        {layout.wires.map((wire) => {
          const target = layout.byKey.get(wire.wire.to.node)!;
          const index = Math.max(1, ORDER.indexOf(target.node.id) === -1 ? 2 : ORDER.indexOf(target.node.id));
          const draw = range(t, CUT + index * 0.18 + 0.08, CUT + index * 0.18 + 0.42);
          const unused = wire.wire.outcome === "up" || wire.wire.outcome === "failed";
          const dim = unused ? 1 - range(t, BLUEPRINT_RUN.checkDown, BLUEPRINT_RUN.checkDown + 0.35) * 0.8 : 1;
          return <path key={wire.wire.id} d={wire.path} pathLength={1} fill="none" stroke="var(--ct-accent)" strokeWidth={1.4} opacity={dim * 0.38} strokeDasharray="1 1" strokeDashoffset={1 - draw} />;
        })}
        {layout.nodes.flatMap((laid, index) => laid.rows.flatMap((row) => {
          const incoming = reaction(t, ARRIVALS[ORDER.indexOf(laid.node.id)]);
          const chosen = laid.node.id === "health" && row.right?.label === "down";
          const outgoing = chosen ? Math.max(branch, reaction(t, 4.2)) : incoming * 0.6;
          return [row.left && <circle key={`${index}-${row.top}-in`} cx={laid.box.x} cy={laid.box.y + row.top + row.height / 2} r={3.2} fill="var(--ct-accent)" opacity={incoming} style={{ filter: "drop-shadow(0 0 4px var(--ct-accent))" }} />,
            row.right && <circle key={`${index}-${row.top}-out`} cx={laid.box.x + laid.box.width} cy={laid.box.y + row.top + row.height / 2} r={chosen ? 3.8 : 3} fill="var(--ct-accent)" opacity={outgoing} style={{ filter: "drop-shadow(0 0 5px var(--ct-accent))" }} />];
        }))}
        {signal && route && <g opacity={Math.min(1, signal.progress * 12, (1 - signal.progress) * 15)} fill="none" stroke="var(--ct-accent)" strokeLinecap="round">
          {[0.16, 0.1, 0.05].map((length, index) => <path key={length} d={route.path} pathLength={1} strokeWidth={3 - index * 0.5} opacity={0.18 + index * 0.2} strokeDasharray={`${Math.min(length, signal.progress)} 2`} strokeDashoffset={-Math.max(0, signal.progress - length)} />)}
          <path d={route.path} pathLength={1} strokeWidth={8} opacity={0.45} style={{ filter: "blur(3px)" }} strokeDasharray="0.002 2" strokeDashoffset={-signal.progress} />
          <path d={route.path} pathLength={1} strokeWidth={3.5} strokeDasharray="0.002 2" strokeDashoffset={-signal.progress} />
        </g>}
      </g>
    </svg>
  </div>;
}

const blueprint: Scene = {
  id: "blueprint", duration: 8,
  render: (t) => t < CUT ? <BlueprintFile t={t} /> : <BlueprintGraph t={t} />,
};

export default blueprint;
