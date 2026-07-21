import { StrictMode, useState } from "react";
import { createRoot } from "react-dom/client";
import { MotionPlayground } from "./MotionPlayground";
import { LiquidGlassLab } from "./concepts/liquidglass/LiquidGlassLab";
import "./playground.css";
import "./lab-switch.css";

const VIEWS = [
  { id: "liquid-glass", label: "Liquid Glass", render: () => <LiquidGlassLab /> },
  { id: "motion-lab", label: "Motion Lab", render: () => <MotionPlayground /> },
] as const;

function initialView(): (typeof VIEWS)[number]["id"] {
  const hash = window.location.hash.replace("#", "");
  return VIEWS.some((v) => v.id === hash) ? (hash as (typeof VIEWS)[number]["id"]) : VIEWS[0].id;
}

function Lab() {
  const [view, setView] = useState(initialView);
  const active = VIEWS.find((v) => v.id === view) ?? VIEWS[0];

  return (
    <>
      <nav className="lab-switch" aria-label="Playground view">
        {VIEWS.map((v) => (
          <button
            key={v.id}
            type="button"
            className={`lab-switch__btn${v.id === view ? " is-active" : ""}`}
            aria-pressed={v.id === view}
            onClick={() => {
              setView(v.id);
              window.location.hash = v.id;
            }}
          >
            {v.label}
          </button>
        ))}
      </nav>
      {active.render()}
    </>
  );
}

const root = document.getElementById("root");
if (!root) throw new Error("Motion Lab root is missing");

createRoot(root).render(
  <StrictMode>
    <Lab />
  </StrictMode>,
);
