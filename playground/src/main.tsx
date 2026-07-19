import { StrictMode, useState, type CSSProperties } from "react";
import { createRoot } from "react-dom/client";
import { MotionPlayground } from "./MotionPlayground";
import { MotionSystemView } from "./MotionSystemView";
import "./playground.css";

const TOGGLE_WRAP: CSSProperties = {
  position: "fixed",
  top: 12,
  left: "50%",
  transform: "translateX(-50%)",
  zIndex: 50,
  display: "flex",
  gap: 4,
  padding: 4,
  borderRadius: 999,
  border: "1px solid #242a33",
  background: "rgb(13 16 22 / 92%)",
  backdropFilter: "blur(12px)",
};

function tabStyle(active: boolean): CSSProperties {
  return {
    padding: "6px 14px",
    borderRadius: 999,
    border: 0,
    cursor: "pointer",
    fontSize: 11,
    letterSpacing: ".04em",
    background: active ? "var(--lab-accent, #58a6ff)" : "transparent",
    color: active ? "#08090c" : "#aeb6c2",
    fontWeight: active ? 700 : 500,
  };
}

function PlaygroundApp() {
  const [view, setView] = useState<"system" | "lab">("system");
  return (
    <>
      <nav style={TOGGLE_WRAP} aria-label="Playground view">
        <button type="button" style={tabStyle(view === "system")} aria-pressed={view === "system"} onClick={() => setView("system")}>
          Motion System
        </button>
        <button type="button" style={tabStyle(view === "lab")} aria-pressed={view === "lab"} onClick={() => setView("lab")}>
          Concept Lab
        </button>
      </nav>
      {view === "system" ? <MotionSystemView /> : <MotionPlayground />}
    </>
  );
}

const root = document.getElementById("root");
if (!root) throw new Error("Motion Lab root is missing");

createRoot(root).render(
  <StrictMode>
    <PlaygroundApp />
  </StrictMode>,
);
