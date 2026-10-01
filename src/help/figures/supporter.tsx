import { Arrow, ArrowMarker, Box, Caption } from "./primitives";

const RECALL_MARKER = "help-arrow-recall";
const TAB_MARKER = "help-arrow-tab-state";

export function SupporterRecallFigure() {
  const sources = [
    { y: 8, label: "Memory", sub: "saved facts" },
    { y: 72, label: "Graph", sub: "linked facts" },
    { y: 136, label: "Docs", sub: "built-in guide" },
  ];
  return (
    <svg
      viewBox="0 0 420 206"
      className="w-full max-w-lg mx-auto"
      role="img"
      aria-label="Memory search with the Supporter Agent: an agent's question gathers candidates from memory, the graph and the docs. The Supporter scores each one, the best come first, and a flag shows when nothing fits well. Results are reordered, never removed."
    >
      <defs>
        <ArrowMarker id={RECALL_MARKER} />
      </defs>
      <Box x={8} y={70} w={84} h={48} label="Question" sub="from an agent" />
      {sources.map((s, i) => (
        <g key={s.label}>
          <Arrow x1={92} y1={90 + i * 4} x2={114} y2={s.y + 22} marker={RECALL_MARKER} />
          <Box x={116} y={s.y} w={92} h={44} label={s.label} sub={s.sub} />
          <Arrow x1={208} y1={s.y + 22} x2={230} y2={84 + i * 10} marker={RECALL_MARKER} />
        </g>
      ))}
      <Box x={232} y={70} w={84} h={48} label="Supporter" sub="scores each" emphasis />
      <Arrow x1={316} y1={88} x2={336} y2={64} marker={RECALL_MARKER} />
      <Arrow x1={316} y1={100} x2={336} y2={132} marker={RECALL_MARKER} dashed />
      <Box x={338} y={40} w={76} h={44} label="Best first" sub="ranked list" emphasis />
      <Box x={338} y={110} w={76} h={44} label="Low match" sub="flagged" dashed />
      <Caption x={210} y={200} text="Results are reordered, never removed" />
    </svg>
  );
}

export function SupporterTabStateFigure() {
  const outcomes = [
    { y: 8, label: "Looks fine", sub: "ready or busy" },
    { y: 66, label: "Weak hint", sub: "not sure", dashed: true },
    { y: 124, label: "Strong signal", sub: "seems stuck", emphasis: true },
  ];
  return (
    <svg
      viewBox="0 0 420 196"
      className="w-full max-w-lg mx-auto"
      role="img"
      aria-label="Tab check with the Supporter Agent: a starting tab's screen and recent context go to the Supporter, which reads it as looks fine, a weak hint, or a strong signal that the tab seems stuck. It is off by default, and CodeTerm's own checks stay in charge."
    >
      <defs>
        <ArrowMarker id={TAB_MARKER} />
      </defs>
      <Box x={8} y={64} w={70} h={48} label="Tab" sub="starting up" />
      <Arrow x1={78} y1={88} x2={96} y2={88} marker={TAB_MARKER} />
      <Box x={98} y={64} w={86} h={48} label="Screen" sub="plus context" />
      <Arrow x1={184} y1={88} x2={202} y2={88} marker={TAB_MARKER} />
      <Box x={204} y={64} w={84} h={48} label="Supporter" sub="reads it" emphasis />
      {outcomes.map((o, i) => (
        <g key={o.label}>
          <Arrow x1={288} y1={80 + i * 8} x2={310} y2={o.y + 22} marker={TAB_MARKER} dashed={o.dashed} />
          <Box x={312} y={o.y} w={100} h={44} label={o.label} sub={o.sub} dashed={o.dashed} emphasis={o.emphasis} />
        </g>
      ))}
      <Caption x={210} y={190} text="Off by default. CodeTerm's own checks stay in charge." />
    </svg>
  );
}
