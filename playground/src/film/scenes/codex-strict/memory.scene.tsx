import { MemoryListTab } from "../../../../../src/components/MemoryPanel/MemoryListTab";
import { API } from "../../../gallery/fixtures";
import { useStage, type Scene } from "../../engine";
import { STRICT_MEMORY_STATES } from "../../fixtures/codex-strict";
import { ProductPlate, StrictStage, StrictTitle } from "./strictStage";

const noop = () => undefined;

function Memory({ t }: { t: number }) {
  const { portrait } = useStage();
  const state = t < 2.4 ? 0 : t < 5 ? 1 : 2;
  const x = portrait ? 108 : 960;
  const y = portrait ? 720 : 384;
  return <StrictStage t={t} number="01" label="MEMORY">
    <StrictTitle lines={["Memory.", "It stays."]} />
    <div style={{ position: "absolute", left: x, top: y - 72, fontSize: 28, color: "var(--ct-muted-fg)" }}>Facts. Notes. Skills.</div>
    <ProductPlate className="strict-memory" x={x} y={y} width={portrait ? 864 : 840} height={528} scale={2.4}>
      <MemoryListTab api={API} nodes={STRICT_MEMORY_STATES[state]} selectedId={null} onSelectItem={noop} onViewInGraph={noop} updateNode={noop} removeNode={noop} />
    </ProductPlate>
    <div style={{ position: "absolute", left: x, top: y + 576, fontSize: 28, color: "var(--ct-accent)", fontVariantNumeric: "tabular-nums" }}>0{state + 1} / retained</div>
  </StrictStage>;
}

const scene: Scene = { id: "cx-strict-memory", duration: 8, render: (t) => <Memory t={t} /> };
export default scene;
