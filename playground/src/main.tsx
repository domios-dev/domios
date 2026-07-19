import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { MotionPlayground } from "./MotionPlayground";
import "./playground.css";

const root = document.getElementById("root");
if (!root) throw new Error("Motion Lab root is missing");

createRoot(root).render(
  <StrictMode>
    <MotionPlayground />
  </StrictMode>,
);
