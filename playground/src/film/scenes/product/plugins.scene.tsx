import "../../fixtures/plugins";
import { useState, type ReactNode } from "react";
import { ExtensionsView } from "../../../../../src/components/Extensions/ExtensionsView";
import { selectExtension } from "../../../../../src/components/Titlebar/extensionsTrayLogic";
import { Camera, Spotlight, useStage, type Scene } from "../../engine";

const noop = () => undefined;
const ACTIVITIES = new Map();
const DISPLAY = new Map();
const W = 1280;
const H = 720;

// World rects of the list rows, measured from the rendered catalog; the narration names them in this order.
const row = (y: number) => ({ x: 112, y, w: 326, h: 40 });
const PEBBLE = row(551);
const TELEGRAM = row(637);
const TEAMS = row(593);
const BITWARDEN = row(421);

function Preselected({ id, children }: { id: string; children: ReactNode }) {
  useState(() => selectExtension(id, noop));
  return children;
}

function Plugins({ t }: { t: number }) {
  const portrait = useStage().portrait;
  return (
    <Camera t={t} width={W} height={H} keys={portrait ? [
      { t: 0, x: 300, y: 420, scale: 2.2 },
      { t: 5, x: 300, y: 430, scale: 2.26 },
    ] : [
      { t: 0, x: W / 2, y: H / 2, scale: 1.5 },
      { t: 5, x: W / 2, y: H / 2, scale: 1.5 },
    ]}>
      <div data-film-hint="Plugins: Pebble, Telegram, Teams, Transcriber, or write your own" style={{ position: "absolute", inset: 0 }}>
        <Preselected id="transcriber">
          <ExtensionsView visible pluginActivities={ACTIVITIES} pluginActivityDisplayMap={DISPLAY} />
        </Preselected>
      </div>
      <Spotlight t={t} radius={10} keys={[
        { t: 1.05, rect: TELEGRAM, dim: 0 },
        { t: 1.25, rect: TELEGRAM },
        { t: 1.52, rect: TELEGRAM },
        { t: 1.62, rect: TEAMS },
        { t: 2.0, rect: TEAMS },
        { t: 2.08, rect: BITWARDEN },
        { t: 2.85, rect: BITWARDEN },
        { t: 2.93, rect: PEBBLE },
        { t: 4.1, rect: PEBBLE },
        { t: 4.45, rect: PEBBLE, dim: 0 },
      ]} />
    </Camera>
  );
}

const plugins: Scene = {
  id: "plugins",
  duration: 5,
  render: (t) => <Plugins t={t} />,
};

export default plugins;
