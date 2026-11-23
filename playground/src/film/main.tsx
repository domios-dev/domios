import "../mocks/setup";
import { useEffect, useState } from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import "../../../src/styles/globals.css";
import "../portal.css";
import { ProductProviders } from "../gallery/fixtures";
import type { Scene } from "./engine";
import { DEFAULT_BACKDROP } from "./backdrops";
import { Frame as StageFrame } from "./embed/frame";
import { applyFilmTheme } from "./embed/theme";
import { AudioPlayer } from "./player";
import { registerScenes } from "./reels/library";

const params = new URLSearchParams(location.search);
const root = document.documentElement;

const applyTheme = (light: boolean) => applyFilmTheme(root, light ? "light" : "dark");
applyTheme(params.get("theme") === "light");
root.style.fontSize = "16px";

const sceneModules = import.meta.glob<{ default: Scene | Scene[] }>("./scenes/**/*.scene.tsx", { eager: true });
// Every scene is already here, so reels play them at once (captures never meet a scene still loading).
registerScenes(sceneModules);
const reelModules = import.meta.glob<{ default: Scene }>("./reels/*.reel.tsx", { eager: true });
const scenes = new Map<string, Scene>();
for (const mod of Object.values(sceneModules)) for (const s of [mod.default].flat()) scenes.set(s.id, s);
for (const mod of Object.values(reelModules)) scenes.set(mod.default.id, mod.default);

const W = Number(params.get("w") ?? 1920);
const H = Number(params.get("h") ?? 1080);
const id = params.get("reel") ?? params.get("scene");
const scene = id ? scenes.get(id) : undefined;
const pulses = ((id && window.__filmCuts?.[id]) || []).slice(1).map((c) => c.start);
const backdrop = params.get("backdrop") ?? DEFAULT_BACKDROP;
const backdropIntensity = Number(params.get("bdi") ?? 0.75);

declare global {
  interface Window {
    __film?: { duration: number; width: number; height: number; seek: (t: number) => Promise<void> };
  }
}

function Index() {
  return (
    <div style={{ padding: 48, color: "var(--ct-fg)", fontSize: 18, lineHeight: 1.8 }}>
      {[...scenes.values()].map((s) => (
        <div key={s.id}>
          <a href={`?scene=${s.id}&play=1`} style={{ color: "var(--ct-accent)" }}>{s.id}</a> {s.duration.toFixed(1)}s
        </div>
      ))}
    </div>
  );
}

function Frame({ scene, t }: { scene: Scene; t: number }) {
  return <StageFrame scene={scene} t={t} width={W} height={H} pulses={pulses} backdrop={backdrop} intensity={backdropIntensity} />;
}

function Stage({ scene }: { scene: Scene }) {
  const [t, setT] = useState(Number(params.get("t") ?? 0));

  useEffect(() => {
    window.__film = {
      duration: scene.duration,
      width: W,
      height: H,
      seek: async (next) => {
        flushSync(() => setT(next));
        for (const a of document.getAnimations()) {
          a.pause();
          a.currentTime = next * 1000;
        }
        await document.fonts.ready;
      },
    };
    if (!params.has("play")) return;
    const start = performance.now() - t * 1000;
    let raf = 0;
    const tick = (now: number) => {
      setT(((now - start) / 1000) % scene.duration);
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, []);

  return <div inert style={{ position: "absolute", inset: 0 }}><Frame scene={scene} t={t} /></div>;
}

const audioSrc = params.get("audio");
const app = !scene ? <Index />
  : audioSrc ? <AudioPlayer src={audioSrc} captions={params.get("captions") ?? undefined} onTheme={applyTheme} light={params.get("theme") === "light"} duration={scene.duration} width={W} height={H} frame={(t) => <Frame scene={scene} t={t} />} />
  : <Stage scene={scene} />;
createRoot(document.getElementById("root")!).render(<ProductProviders>{app}</ProductProviders>);
