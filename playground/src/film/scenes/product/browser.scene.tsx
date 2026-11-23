import { BROWSER_STATE, DocsPage, PICKED_COMMAND } from "../../fixtures/browserPage";
import { BROWSER_SURFACE_CHROME, BROWSER_SURFACE_TITLE } from "../../../../../src/components/Surface/browserChrome";
import { PointAndAsk } from "../../../../../src/components/Surface/PointAndAsk";
import { resolveToolbar, surfaceLabel } from "../../../../../src/components/Surface/surfaceChromeModel";
import { SurfaceToolbar } from "../../../../../src/components/Surface/SurfaceToolbar";
import { Camera, Reveal, useStage, type Scene } from "../../engine";

const noop = () => undefined;
const LABEL = surfaceLabel(BROWSER_STATE.owner, BROWSER_SURFACE_TITLE);
const PICK_AT = 1.7;
// Stage-relative top just under the picked command block, measured from the rendered page.
const PROMPT_TOP = 516;

function Browser({ t }: { t: number }) {
  const picking = t >= PICK_AT;
  const portrait = useStage().portrait;
  const items = resolveToolbar(BROWSER_SURFACE_CHROME, { interactive: true, picking, state: BROWSER_STATE, busy: new Set() });
  return (
      <Camera t={t} width={1280} height={720} keys={portrait ? [
        { t: 0, x: 640, y: 540, scale: 0.72 },
        { t: 0.9, x: 640, y: 540, scale: 0.72 },
        { t: 1.35, x: 640, y: 540, scale: 0.72 },
        { t: 4, x: 640, y: 540, scale: 0.72 },
      ] : [
        { t: 0, x: 640, y: 360, scale: 1.5 },
        { t: 0.9, x: 640, y: 360, scale: 1.5 },
        { t: 1.35, x: 660, y: 492, scale: 2.25 },
        { t: 4, x: 660, y: 494, scale: 2.3 },
      ]}>
        <section className="ct-surface" aria-label={LABEL} style={{ position: "absolute", inset: 0 }}>
          <SurfaceToolbar label={LABEL} items={items} onButton={noop} onToggle={noop} onSubmit={noop} onSelect={noop} />
          <div className="ct-surface-stage">
            <div className="ct-surface-canvas" data-mode={picking ? "picking" : "interactive"} aria-label={BROWSER_STATE.title}>
              <DocsPage picking={picking} />
            </div>
            <Reveal t={t} at={PICK_AT + 0.4} dur={0.35} dy={14}>
              <PointAndAsk
                owner={BROWSER_STATE.owner}
                subject={{ kind: "element", card: { kind: "text", name: `Code "${PICKED_COMMAND.split(" \\")[0]}"`, path: "DocsPage · main > pre" } }}
                position={{ left: 262, top: PROMPT_TOP }}
                onSend={noop}
                onCancel={noop}
              />
            </Reveal>
          </div>
        </section>
      </Camera>
  );
}

const browser: Scene = {
  id: "browser",
  duration: 4,
  render: (t) => <Browser t={t} />,
};

export default browser;
